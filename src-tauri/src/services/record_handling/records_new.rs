use super::utils::sync_transaction;
use crate::services::local_storage::{
    session::view::VIEWSESSION,
    storage_api::transaction::get_unconfirmed_and_failed_transaction_ids,
};
use avail_common::errors::{AvailError, AvailErrorType, AvailResult};
use chrono::{DateTime, Local};
use serde_json::Value;
use snarkvm::ledger::transactions::ConfirmedTransaction;
use snarkvm::prelude::{
    Field, FromStr, Group, Network, Parser, Scalar, Serialize, Testnet3, ToField, Transaction,
};
use snarkvm::synthesizer::program::FinalizeOperation;
use tauri_plugin_http::reqwest::Client;
use tokio::runtime::Runtime;

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
    transaction: Transaction<N>,
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
    network_id: String,
}

/// Implementation of the LocalClient struct.
impl LocalClient {
    fn new(api_key: String, base_url: String, network_id: String) -> Self {
        LocalClient {
            client: tauri_plugin_http::reqwest::Client::new(),
            api_key: api_key,
            base_url: base_url,
            network_id: network_id,
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
    fn get_records(&self, start: u32, end: u32) -> AvailResult<Vec<Value>> {
        let url = format!(
            "{}/api/{}/record/ownership/heightRange?start={}&end={}",
            self.base_url, self.api_key, start, end
        );
        let rt = Runtime::new().unwrap();
        let request = self.client.get(url);
        let response = rt.block_on(request.send())?;

        // Get content from response
        let content = rt.block_on(response.text())?;

        // Parse the content as JSON
        Ok(serde_json::from_str::<Value>(&content)?
            .as_array().unwrap()
            .clone())
    }

    // Return a transaction
    fn get_transaction_id_from_transition(&self, transition_id: &str) -> AvailResult<String> {
        let url: String = format! {
            "{}/v1/{}/{}/find/transactionID/{transition_id}",
            self.base_url, self.api_key, self.network_id
        };
        let rt = Runtime::new().unwrap();
        let request = self.client.get(url);
        let response = rt.block_on(request.send())?;

        // Get content from response
        let content = rt.block_on(response.text())?;

        // Parse the content as JSON
        Ok(serde_json::from_str::<String>(&content)?)
    }

    fn get_transaction<N: Network>(&self, transaction_id: &str) -> AvailResult<Transaction<N>> {
        let url = format!(
            "{}/v1/{}/{}/transaction/{transaction_id}",
            self.base_url, self.api_key, self.network_id
        );
        let rt = Runtime::new().unwrap();
        let request = self.client.get(url);
        let response = rt.block_on(request.send())?;

        // Get content from response
        let content = rt.block_on(response.text())?;

        // Parse the content as JSON
        Ok(serde_json::from_str::<Transaction<N>>(&content)?)
    }

    fn get_block_from_transaction_id(&self, transaction_id: &str) -> AvailResult<Value> {
        let mut url = format!(
            "{}/v1/{}/{}/find/blockHash/{transaction_id}",
            self.base_url, self.api_key, self.network_id
        );
        let rt = Runtime::new().unwrap();
        let mut request = self.client.get(url);
        let mut response = rt.block_on(request.send())?;

        // Get content from response
        let mut content = rt.block_on(response.text())?;

        // Parse the content as JSON
        let block_hash = serde_json::from_str::<String>(&content)?;

        url = format!(
            "{}/v1/{}/{}/block/{block_hash}",
            self.base_url, self.api_key, self.network_id
        );
        request = self.client.get(url);
        response = rt.block_on(request.send())?;

        // Get content from response
        content = rt.block_on(response.text())?;

        // Parse the content as JSON
        Ok(serde_json::from_str::<Value>(&content)?)
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
fn is_owner_direct<N: Network>(
    address_x_coordinate: Field<N>,
    view_key_scalar: Scalar<N>,
    record_nonce: Group<N>,
    record_owner_x_coordinate: Field<N>,
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
        let (_, nonce_x) =
            Field::<N>::parse(record.get("nonce_x").unwrap().as_str().unwrap()).unwrap();
        let (_, nonce_y) =
            Field::<N>::parse(record.get("nonce_y").unwrap().as_str().unwrap()).unwrap();
        let (_, owner_x) =
            Field::<N>::parse(record.get("owner_x").unwrap().as_str().unwrap()).unwrap();
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
        finalize_operations,
    )
    .unwrap()
}

/// Add to sync transaction parameters.
pub fn convert_txn_to_confirmed_txn<N: Network>(
    transaction: Transaction<N>
) -> AvailResult<ConfirmedTransaction<N>>{
    let client = LocalClient::new(
        env!("TESTNET_API_OBSCURA").to_string(),
        "https://aleo-testnet3.obscura.network".to_string(),
        "testnet3".to_string(),
    );
    let transaction_id = transaction.id();
    let transaction_id_str = transaction_id.clone().to_string();
    let block = client.get_block_from_transaction_id(&transaction_id_str)?;

    let transactions = block.get("transactions").unwrap().as_array().unwrap();

    let mut res: Option<ConfirmedTransaction<N>> = None;
    for txn in transactions {
        let txn_id = txn
            .get("transaction")
            .unwrap()
            .get("id")
            .unwrap()
            .to_string();

        if let Ok(txn_id_obj) = N::TransactionID::from_str(&txn_id) {
            if txn_id_obj == transaction_id {
                res = Some(convert_to_confirmed_transaction::<N>(txn.clone()));
                break;
            }
        };
    }
    if res.is_some() {
        Ok(res.unwrap())
    } else {
        Err(AvailError::new(
            AvailErrorType::NotFound,
            "Transaction not found".to_string(),
            "Transaction not found".to_string()
        ))
    }
}

/**
 * Get owned transactions.
 * @param transitions The transitions to get the transactions from.
 * @param client The client for the API requests.
 * @return The owned transactions.
 */
fn convert_to_sync_txn_params<N: Network>(
    transitions: Vec<String>,
    client: &LocalClient,
) -> AvailResult<Vec<SyncTxnParams<N>>> {
    let mut sync_txn_params: Vec<SyncTxnParams<N>> = Vec::new();
    let rt = Runtime::new().unwrap();

    for transition in transitions {
        // Get block
        let transaction_id = client.get_transaction_id_from_transition(&transition)?;
        let transaction = client.get_transaction::<N>(&transaction_id)?;
        let block = client.get_block_from_transaction_id(&transaction_id)?;

        // Get block height
        let block_height = block
            .get("header")
            .unwrap()
            .get("metadata")
            .unwrap()
            .get("height")
            .unwrap()
            .as_u64()
            .unwrap() as u32;

        // Get timestamp
        let timestamp = block
            .get("header")
            .unwrap()
            .get("metadata")
            .unwrap()
            .get("timestamp")
            .unwrap()
            .as_i64()
            .unwrap();

        sync_txn_params.push(SyncTxnParams {
            transaction,
            block_height,
            timestamp: DateTime::from_timestamp(timestamp, 0)
                .unwrap()
                .with_timezone(&Local),
        });
    }
    Ok(sync_txn_params)
}

/**
 * Get records.
 * @param start The start block.
 * @param end The end block.
 * @return The records.
 */
pub fn get_records_new<N: Network>(start: u32, end: u32) -> AvailResult<(Vec<Value>)> {
    // Prepare API client and get records
    // ATTENTION: Different API and base_url to get records
    let api_key: String = "bcde0fb4-a4fa-4e84-affd-ab70b5e477db".to_string();
    let client = LocalClient::new(
        api_key,
        "https://aleo-testnet3.dev.obscura.network".to_string(),
        "testnet3".to_string(),
    );
    let records = client.get_records(start, end)?;
    Ok(records)
}

/**
 * Get sync transaction parameters.
 * @param records The records to get the sync transaction parameters from.
 * @return The sync transaction parameters.
 */
pub fn get_sync_txn_params<N: Network>(
    records: Vec<Value>,
) -> AvailResult<(Vec<(SyncTxnParams<N>)>)> {
    // Get transitions from owned records
    let transitions = owned_records_to_transitions::<N>(records);

    // Prepare API client
    let api_key = env!("TESTNET_API_OBSCURA").to_string();
    let client = LocalClient::new(
        api_key,
        "https://aleo-testnet3.obscura.network".to_string(),
        "testnet3".to_string(),
    );

    // Get sync transaction parameters
    let sync_txn_params = convert_to_sync_txn_params(transitions, &client)?;

    Ok((sync_txn_params))
}

#[cfg(test)]
mod record_handling_tests {
    use super::*;
    use crate::models::pointers::record::AvailRecord;
    use avail_common::models::encrypted_data::EncryptedData;

    #[test]
    fn test_get_records() {
        type N = Testnet3;

        let view_key = env!("VIEW_KEY");
        VIEWSESSION.set_view_session(view_key).unwrap();
        let current_block: u32 = 2087986;
        let start: u32 = current_block - 100;
        let records = get_records_new::<N>(start, current_block);

        assert!(records.is_ok());
    }

    #[test]
    fn test_get_sync_txn_params() {
        type N = Testnet3;

        let view_key = env!("VIEW_KEY");
        VIEWSESSION.set_view_session(view_key).unwrap();
        let current_block: u32 = 2087986;
        let start: u32 = current_block - 10;
        let records = get_records_new::<N>(start, current_block).unwrap();

        let sync_txn_params = get_sync_txn_params::<N>(records);

        assert!(sync_txn_params.is_ok());
    }

    #[test]
    fn test_sync_transaction() {
        type N = Testnet3;

        let view_key = env!("VIEW_KEY");
        VIEWSESSION.set_view_session(view_key).unwrap();
        let current_block: u32 = 2087986;
        let start: u32 = current_block - 10;
        let records = get_records_new::<N>(start, current_block).unwrap();

        let sync_txn_params = get_sync_txn_params::<N>(records).unwrap();

        let mut res: Vec<
            AvailResult<(
                Option<EncryptedData>,
                Vec<AvailRecord<N>>,
                Vec<EncryptedData>,
                bool,
            )>,
        > = Vec::new();

        // Sync transactions
        for params in sync_txn_params {
            res.push(sync_transaction::<Testnet3>(
                &params.transaction,
                params.block_height,
                params.timestamp,
                None,
                None,
            ));
        }
        assert!(res[0].is_ok());
    }
}
