use super::utils::sync_transaction;
use crate::{api::aleo_client::setup_client, services::local_storage::session::view::VIEWSESSION};
use avail_common::errors::{AvailError, AvailErrorType, AvailResult};
use chrono::{DateTime, Local};
use libc::time;
use serde_json::Value;
use snarkvm::ledger::transactions::ConfirmedTransaction;
use snarkvm::prelude::{
    Block, Field, FromStr, Group, Network, Parser, Scalar, Serialize, Testnet3, ToField,
    Transaction,
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
pub(crate) struct SyncTxnParams<N: Network> {
    pub(crate) transaction: ConfirmedTransaction<N>,
    pub(crate) block_height: u32,
    pub(crate) timestamp: DateTime<Local>,
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
            .as_array()
            .unwrap()
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

    fn get_block_from_transaction_id<N: Network>(
        &self,
        transaction_id: &str,
    ) -> AvailResult<Block<N>> {
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
        Ok(serde_json::from_str::<Block<N>>(&content)?)
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
    let owner_x = record_owner_x_coordinate - randomizer[0];
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

            // Check if transition ID is already stored in encrypted_data table

            transitions.push(transition_id.to_string());
        };
    }
    transitions
}

/**
 * Convert Transaction type to ConfirmedTransaction type
 * Note: Redundant API call to be optimised in future with better way
 *
 * Finalize information which is necessary to convert
 *  only contains in get_block API but not get_transaction API
 */
pub fn convert_txn_to_confirmed_txn<N: Network>(
    transaction: Transaction<N>,
) -> AvailResult<ConfirmedTransaction<N>> {
    let client = LocalClient::new(
        env!("TESTNET_API_OBSCURA").to_string(),
        "https://aleo-testnet3.obscura.network".to_string(),
        "testnet3".to_string(),
    );

    let transaction_id = transaction.id();
    let block = client.get_block_from_transaction_id::<N>(&transaction_id.to_string())?;

    let transactions = block.transactions();

    // Search for matching transaction from block
    let mut res: Option<ConfirmedTransaction<N>> = None;
    for confirmed_txn in transactions.iter() {
        if confirmed_txn.id() == transaction_id {
            // res = Some(convert_to_confirmed_transaction::<N>(txn.clone(), transaction)?);
            println!("Confirmed Transaction\n{:?}\n", confirmed_txn);
            res = Some(confirmed_txn.clone());
            break;
        }
    }
    if res.is_some() {
        Ok(res.unwrap())
    } else {
        Err(AvailError::new(
            AvailErrorType::NotFound,
            "Transaction not found".to_string(),
            "Transaction not found".to_string(),
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
        let block: Block<N> = client.get_block_from_transaction_id(&transaction_id)?;

        // Populate sync transaction parameters
        let block_height = block.height();
        let timestamp = block.timestamp();
        let mut confirmed_transaction: Option<ConfirmedTransaction<N>> = None;

        // Search for matching transaction from block
        let transactions = block.transactions();
        for confirmed_txn in transactions.iter() {
            if confirmed_txn.id().to_string() == transaction_id {
                println!("Found confirmed transaction:\n{:?}\n", confirmed_txn);
                confirmed_transaction = Some(confirmed_txn.clone());
                break;
            }
        }

        // Add sync transaction parameters
        sync_txn_params.push(SyncTxnParams {
            transaction: confirmed_transaction.unwrap(),
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
        let current_block: u32 = 2249244;
        let start: u32 = current_block - 10;
        let records = get_records_new::<N>(start, current_block).unwrap();

        // TODO : Check if the record is already stored

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
        println!("Result of sync_transaction:\n{:?}\n", res);
        assert!(res[0].is_ok());
    }
}
