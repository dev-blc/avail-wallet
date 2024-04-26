use avail_common::errors::AvailResult;
use chrono::{DateTime, Local};
use crate::services::local_storage::{session::view::VIEWSESSION, storage_api::transaction::get_unconfirmed_and_failed_transaction_ids};
use snarkvm::prelude::{
	Network, Testnet3, ToField, FromStr, Parser, Field, Group, Scalar, Transaction, Serialize
};
use snarkvm::ledger::transactions::ConfirmedTransaction;
use snarkvm::synthesizer::program::FinalizeOperation;
use serde_json::Value;
use tauri_plugin_http::reqwest::Client;
use super::utils::sync_transaction;

/**
 * SyncTxnParams struct
 *
 * A struct that holds the parameters for the sync transaction.
 *
 * transaction: The transaction to sync.
 * block_height: The height of the block.
 * timestamp: The timestamp of the block.
 */
#[derive(Debug, Clone)]
struct SyncTxnParams<N: Network> {
	transaction: ConfirmedTransaction<N>,
	block_height: u32,
	timestamp: DateTime<Local>,
}
/**
 * LocalClient struct
 *
 * A struct that holds the client for the API requests.
 *
 * client: The client for the API requests.
 * api_key: The API
 * base_url: The base URL for the API requests.
 * network_id: The network ID for the API requests.
 */
struct LocalClient {
	client: Client,
	api_key: String,
	base_url: String,
	network_id: String
}

/// Implementation of the LocalClient struct.
impl LocalClient {
	fn new(api_key: String, base_url: String, network_id: String) -> Self {
		LocalClient {
			client: tauri_plugin_http::reqwest::Client::new(),
			api_key: api_key,
			base_url: base_url,
			network_id: network_id
		}
	}

	fn change_api_key(&mut self, api_key: String) {
		self.api_key = api_key;
	}

	fn change_base_url(&mut self, base_url: String) {
		self.base_url = base_url;
	}

	fn change_network_id(&mut self, network_id: String) {
		self.network_id = network_id;
	}

	// Return arrays of records
	async fn get_records(&self, start: u32, end: u32) -> Vec<Value> {
		let url = format!(
			"{}/api/{}/record/ownership/heightRange?start={}&end={}",
			self.base_url,
			self.api_key, 
			start, 
			end
		);
		let response = self.client.get(url).send().await.unwrap();

		// Get content from response
		let content = response.text().await.unwrap();

		// Parse the content as JSON
		serde_json::from_str::<Value>(&content).unwrap().as_array().unwrap().clone()
	}

	// Return a transaction
	async fn get_transaction_id_from_transition(&self, transition_id: &str) -> String {
		let url: String = format!{
			"{}/v1/{}/{}/find/transactionID/{transition_id}",
			self.base_url, self.api_key, self.network_id
		};
		let response = self.client.get(url).send().await.unwrap();

		// Get content from response
		let content = response.text().await.unwrap();

		// Parse the content as JSON
		serde_json::from_str::<String>(&content).unwrap()
	}

	async fn get_transaction(&self, transaction_id: &str) -> Value {
        let url = format!(
            "{}/v1/{}/{}/transaction/{transaction_id}",
            self.base_url, self.api_key, self.network_id
        );
		let response = self.client.get(url).send().await.unwrap();

		// Get content from response
		let content = response.text().await.unwrap();

		// Parse the content as JSON
		serde_json::from_str::<Value>(&content).unwrap()
	}

	async fn get_block_from_transaction_id(&self, transaction_id: &str) -> Value {
		let mut url = format!(
			"{}/v1/{}/{}/find/blockHash/{transaction_id}",
			self.base_url, self.api_key, self.network_id
		);
		let mut response = self.client.get(url).send().await.unwrap();

		// Get content from response
		let mut content = response.text().await.unwrap();

		// Parse the content as JSON
		let block_hash = serde_json::from_str::<String>(&content).unwrap();

		url = format!(
			"{}/v1/{}/{}/block/{block_hash}",
			self.base_url, self.api_key, self.network_id
		);
		response = self.client.get(url).send().await.unwrap();

		// Get content from response
		content = response.text().await.unwrap();

		// Parse the content as JSON
		serde_json::from_str::<Value>(&content).unwrap()
	}
}

/**
 * Check if the address is the owner of the record.
 * @param address_x_coordinate The x-coordinate of the address.
 * @param view_key_scalar The scalar of the view key.
 * @param record_nonce The nonce of the record.
 * @param record_owner_x_coordinate The x-coordinate of the owner.
 * @return True if the address is the owner of the record, false otherwise.
 */
fn is_owner_direct<N:Network>(
    address_x_coordinate: Field<N>,
    view_key_scalar: Scalar<N>,
    record_nonce: Group<N>,
    record_owner_x_coordinate: Field<N>
) -> bool {
    let record_view_key = (record_nonce * view_key_scalar).to_x_coordinate();
    // Compute the 0th randomizer.
    let randomizer = N::hash_many_psd8(&[N::encryption_domain(), record_view_key], 1);
    // Decrypt the owner.
    let owner_x = record_owner_x_coordinate - &randomizer[0];
    // Check if the address is the owner.
    owner_x == address_x_coordinate
}

/**
 * Convert owned records to transitions.
 * @param records The records to convert.
 * @return The transitions.
 */
fn owned_records_to_transitions<N: Network>(records: Vec<Value>) -> Vec<String> {
	let mut transitions = Vec::new();
	for record in records {
		// Get values from record and cast to primitive types
		let (_, nonce_x) = Field::<N>::parse(record.get("nonce_x").unwrap().as_str().unwrap()).unwrap();
		let (_, nonce_y) = Field::<N>::parse(record.get("nonce_y").unwrap().as_str().unwrap()).unwrap();
		let (_, owner_x) = Field::<N>::parse(record.get("owner_x").unwrap().as_str().unwrap()).unwrap();
		let nonce = Group::<N>::from_xy_coordinates(nonce_x, nonce_y);
		let view_key = VIEWSESSION.get_instance::<N>().unwrap();
		let address = view_key.to_address().to_field().unwrap();

		// Check if the record is owned
		if (is_owner_direct(address, *view_key, nonce, owner_x)) {
			println!("Found record owned:\n{}\n", record);

			// Get transition ID
			let transition_id = record.get("transition_id").unwrap().as_str().unwrap();
			transitions.push(transition_id.to_string());
		};
	}
	transitions
}

/// Convert transaction to confirmed transaction.
fn convert_to_confirmed_transaction<N: Network>(transaction: Value) -> ConfirmedTransaction<N> {
	let mut finalize_operations: Vec<FinalizeOperation<N>> = Vec::new();

	// Get finalizes from transaction
	let finalizes = transaction.get("finalize").unwrap().as_array().unwrap();
	for finalize in finalizes {
		finalize_operations.push(FinalizeOperation::from_str(&finalize.to_string()).unwrap());
	}

	// Return as confirmed transaction
	ConfirmedTransaction::accepted_execute(
		transaction.get("index").unwrap().as_u64().unwrap() as u32,
		Transaction::from_str(&transaction.get("transaction").unwrap().to_string()).unwrap(),
		finalize_operations
	).unwrap()
}

/// Add to sync transaction parameters.
fn add_to_sync_txn_params<N: Network>(
	block: &Value,
	block_height: u32,
	timestamp: i64,
	transition: &str,
	sync_txn_params: &mut Vec<SyncTxnParams<N>>
) {
	// Get transactions from block
	let transactions = block.get("transactions").unwrap().as_array().unwrap();

	// Filter owned transaction only
	for txn in transactions {
		// Get transitions from transaction
		let local_transitions = txn
			.get("transaction").unwrap()
			.get("execution").unwrap()
			.get("transitions").unwrap()
			.as_array().unwrap();

		for tsn in local_transitions {
			let id = tsn
				.get("id").unwrap()
				.as_str().unwrap();

			// Check if it's owned transition
			if (id == transition) {
				println!("Found new owned transition:\n{} == {}\n", id, transition);
				sync_txn_params.push(
					SyncTxnParams {
						transaction: convert_to_confirmed_transaction::<N>(txn.clone()),
						block_height,
						timestamp: DateTime::from_timestamp(timestamp, 0).unwrap().with_timezone(&Local)
					}
				);
				println!("Added to sync transaction parameters:\n{}\n", txn);
			}
		}
	}
}

/**
 * Get owned transactions.
 * @param transitions The transitions to get the transactions from.
 * @param client The client for the API requests.
 * @return The owned transactions.
 */
async fn convert_to_sync_txn_params<N: Network>(
	transitions: Vec<String>,
	client: &LocalClient
) -> Vec<SyncTxnParams<N>> {
	let mut sync_txn_params: Vec<SyncTxnParams<N>> = Vec::new();

	for transition in transitions {
		// Get block
		let transaction_id = client.get_transaction_id_from_transition(&transition).await;
		let block = client.get_block_from_transaction_id(&transaction_id).await;

		// Get block height
		let block_height = block
			.get("header").unwrap()
			.get("metadata").unwrap()
			.get("height").unwrap()
			.as_u64().unwrap() as u32;

		// Get timestamp
		let timestamp = block
			.get("header").unwrap()
			.get("metadata").unwrap()
			.get("timestamp").unwrap()
			.as_i64().unwrap();
		add_to_sync_txn_params(&block, block_height, timestamp, &transition, &mut sync_txn_params);
	}
	sync_txn_params
}

/**
 * Get records.
 * @param start The start block.
 * @param end The end block.
 * @return The records.
 */
pub async fn get_records_new<N: Network>(
    start: u32,
    end: u32
) -> AvailResult<(Vec<Value>)> {
    // Prepare API client and get records
	// ATTENTION: Different API and base_url to get records
    let api_key: String = "bcde0fb4-a4fa-4e84-affd-ab70b5e477db".to_string();
	let client = LocalClient::new(
		api_key, 
		"https://aleo-testnet3.dev.obscura.network".to_string(), 
		"testnet3".to_string()
	);
    let records = client.get_records(start, end).await;
    Ok((records))
}

/**
 * Get sync transaction parameters.
 * @param records The records to get the sync transaction parameters from.
 * @return The sync transaction parameters.
 */
pub async fn get_sync_txn_params<N: Network>(records: Vec<Value>) -> AvailResult<(Vec<(SyncTxnParams<N>)>)> {
	// Get transitions from owned records
	let transitions = owned_records_to_transitions::<N>(records);

	// Prepare API client
	let api_key = env!("TESTNET_API_OBSCURA").to_string();
	let client = LocalClient::new(
		api_key,
		"https://aleo-testnet3.obscura.network".to_string(),
		"testnet3".to_string()
	);

	// Get sync transaction parameters
	let sync_txn_params = convert_to_sync_txn_params(transitions, &client).await;

	Ok((sync_txn_params))
}

/// WIP
pub fn check_unconfirmed_transactions() -> AvailResult<()> {
	let unconfimred_and_failed_transactions = get_unconfirmed_and_failed_transaction_ids::<Testnet3>()?;

	Ok(())
}

#[cfg(test)]
mod record_handling_tests {
	use avail_common::models::encrypted_data::EncryptedData;
	use crate::models::pointers::record::AvailRecord;
	use super::*;

	#[tokio::test]
	async fn test_get_records() {
		type N = Testnet3;

		let view_key = env!("VIEW_KEY");
		VIEWSESSION.set_view_session(view_key).unwrap();
		let current_block: u32 = 2087986;
		let start: u32 = current_block - 100;
		let records= get_records_new::<N>(start, current_block).await;

		assert!(records.is_ok());
	}

	#[tokio::test]
	async fn test_get_sync_txn_params() {
		type N = Testnet3;

		let view_key = env!("VIEW_KEY");
		VIEWSESSION.set_view_session(view_key).unwrap();
		let current_block: u32 = 2087986;
		let start: u32 = current_block - 10;
		let records= get_records_new::<N>(start, current_block).await;

		let sync_txn_params = get_sync_txn_params::<N>(records.unwrap()).await;

		assert!(sync_txn_params.is_ok());

		for i in sync_txn_params.unwrap() {
			println!("SyncTxnParams: {:?}", i);
		}
	}

	#[tokio::test]
	async fn test_sync_transaction() {
		type N = Testnet3;

		let view_key = env!("VIEW_KEY");
		VIEWSESSION.set_view_session(view_key).unwrap();
		let current_block: u32 = 2087986;
		let start: u32 = current_block - 10;
		let records= get_records_new::<N>(start, current_block).await;

		let sync_txn_params = get_sync_txn_params::<N>(records.unwrap()).await.unwrap();

		let mut res: Vec<AvailResult<(
			Option<EncryptedData>,
			Vec<AvailRecord<N>>,
			Vec<EncryptedData>,
			bool,
		)>> = Vec::new();

		// Sync transactions
		for params in sync_txn_params {
			res.push(sync_transaction::<Testnet3>(
				&params.transaction,
				params.block_height,
				params.timestamp,
				None,
				None
			));
		}
		assert!(res[0].is_ok());
	}
}