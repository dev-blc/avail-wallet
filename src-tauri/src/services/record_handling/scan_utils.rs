use super::utils::{handle_deployment_confirmed, handle_deployment_rejection, sync_transaction};

use crate::services::local_storage::storage_api::{
    deployment::get_deployment_pointer,
    transaction::{
        check_unconfirmed_transactions, get_transaction_pointer,
        get_unconfirmed_and_failed_transaction_ids
    },
    transition::is_transition_stored,
};
use crate::services::record_handling::utils::{
    get_executed_transitions, handle_transaction_confirmed, handle_transaction_rejection,
    input_spent_check, transition_to_record_pointer,
};
use crate::{
    models::pointers::transition::{TransitionPointer, TransitionType},
    helpers::utils::get_timestamp_from_i64,
    services::local_storage::{
        encrypted_data::handle_block_scan_failure, session::view::VIEWSESSION,
    },
};

use avail_common::errors::{AvailError, AvailErrorType, AvailResult};
use chrono::{DateTime, Local};

use serde_json::Value;
use snarkvm::ledger::transactions::ConfirmedTransaction;
use snarkvm::prelude::{
    Block, Field, FromStr, Group, Network, Parser, Scalar, Serialize, Testnet3, ToField,
    Transaction, Address
};

use tauri::{Manager, Window};
use tauri_plugin_http::reqwest::Client;
use crate::services::local_storage::encrypted_data::store_encrypted_data;
use crate::models::pointers::transaction::TransactionPointer;

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

    async fn get_pub_tsn_by_addr_and_height(
        &self,
        addr: &str,
        start: u32,
        end: u32,
        limit: u32
    ) -> AvailResult<Vec<Value>> {
        let url = format!(
            "{}/api/{}/transitions/address/{}/blocks?start={}&end={}&limit={}",
            self.base_url, self.api_key, addr, start, end, limit
        );

        let request = self.client.get(url);
        let response = request.send().await?;

        // Get content from response
        let content = response.text().await?;
        println!("Content: \n{}\n", content);

        match serde_json::from_str::<Value>(&content)?.as_array() {
            Some(transactions) => Ok(transactions.clone()),
            None => Err(AvailError::new(
                AvailErrorType::Internal,
                "Error parsing transactions".to_string(),
                "No public transitions found".to_string(),
            ))
        }
    }

    // Return arrays of records
    async fn get_records(&self, start: u32, end: u32) -> AvailResult<Vec<Value>> {
        let url = format!(
            "{}/api/{}/record/ownership/heightRange?start={}&end={}",
            self.base_url, self.api_key, start, end
        );

        let request = self.client.get(url);
        let response = request.send().await?;

        // Get content from response
        let content = response.text().await?;

        // Parse the content as JSON
        Ok(serde_json::from_str::<Value>(&content)?
            .as_array()
            .unwrap()
            .clone())
    }

    // Return a transaction
    async fn get_transaction_id_from_transition(&self, transition_id: &str) -> AvailResult<String> {
        let url: String = format! {
            "{}/v1/{}/{}/find/transactionID/{transition_id}",
            self.base_url, self.api_key, self.network_id
        };

        let request = self.client.get(url);
        let response = request.send().await?;

        // Get content from response
        let content = response.text().await?;

        // Parse the content as JSON
        Ok(serde_json::from_str::<String>(&content)?)
    }

    async fn get_transaction<N: Network>(
        &self,
        transaction_id: &str,
    ) -> AvailResult<Transaction<N>> {
        let url = format!(
            "{}/v1/{}/{}/transaction/{transaction_id}",
            self.base_url, self.api_key, self.network_id
        );

        let request = self.client.get(url);
        let response = request.send().await?;

        // Get content from response
        let content = response.text().await?;

        // Parse the content as JSON
        Ok(serde_json::from_str::<Transaction<N>>(&content)?)
    }

    async fn get_block_from_transaction_id<N: Network>(
        &self,
        transaction_id: &str,
    ) -> AvailResult<Block<N>> {
        let mut url = format!(
            "{}/v1/{}/{}/find/blockHash/{transaction_id}",
            self.base_url, self.api_key, self.network_id
        );

        let mut request = self.client.get(url);
        let mut response = request.send().await?;

        // Get content from response
        let mut content = response.text().await?;

        // Parse the content as JSON
        let block_hash = serde_json::from_str::<String>(&content)?;

        url = format!(
            "{}/v1/{}/{}/block/{block_hash}",
            self.base_url, self.api_key, self.network_id
        );
        request = self.client.get(url);
        response = request.send().await?;

        // Get content from response
        content = response.text().await?;

        // Parse the content as JSON
        Ok(serde_json::from_str::<Block<N>>(&content)?)
    }
}

pub async fn get_block_from_transaction_id<N: Network>(
    transaction_id: &str,
) -> AvailResult<Block<N>> {
    let client = LocalClient::new(
        env!("TESTNET_API_OBSCURA").to_string(),
        "https://aleo-testnet3.obscura.build".to_string(),
        "testnet3".to_string(),
    );

    let block = client
        .get_block_from_transaction_id::<N>(transaction_id)
        .await?;
    Ok(block)
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
fn owned_records_to_transitions<N: Network>(
    records: Vec<Value>,
    window: Option<Window>,
) -> AvailResult<Vec<String>> {
    let mut transitions = Vec::new();

    let records_len = records.len();

    for (index, record) in records.iter().enumerate() {
        /* -- Calculate Scan Progress Percentage-- */
        let percentage = ((index as f32 / records_len as f32) * 10000.0).round() / 100.0;

        let percentage = if percentage > 100.0 {
            100.0
        } else {
            percentage
        };

        if let Some(window) = window.clone() {
            match window.emit("scan_progress", percentage) {
                Ok(_) => {}
                Err(e) => {
                    return Err(AvailError::new(
                        AvailErrorType::Internal,
                        e.to_string(),
                        "Error updating progress bar".to_string(),
                    ))
                }
            };
        }

        // Get values from record and cast to primitive types
        let (_, nonce_x) =
            Field::<N>::parse(record.get("nonce_x").unwrap().as_str().unwrap()).unwrap();
        let (_, nonce_y) =
            Field::<N>::parse(record.get("nonce_y").unwrap().as_str().unwrap()).unwrap();
        let (_, owner_x) =
            Field::<N>::parse(record.get("owner_x").unwrap().as_str().unwrap()).unwrap();
        let nonce = Group::<N>::from_xy_coordinates(nonce_x, nonce_y);
        let view_key = VIEWSESSION.get_instance::<N>()?;
        let address = view_key.to_address().to_field()?;

        // Check if the record is owned
        if (is_owner_direct(address, *view_key, nonce, owner_x)) {
            println!("Found record owned:\n{}\n", record);

            // Get transition ID
            let transition_id = record.get("transition_id").unwrap().as_str().unwrap();

            // Check if transition ID is already stored in encrypted_data table

            let is_stored = match is_transition_stored(transition_id) {
                Ok(res) => res,
                Err(e) => return Err(e),
            };

            match is_stored {
                true => {
                    println!("Transition already stored\n");
                }
                false => {
                    transitions.push(transition_id.to_string());
                }
            }
        };
    }

    Ok(transitions)
}

/**
 * Convert Transaction type to ConfirmedTransaction type
 * Note: Redundant API call to be optimised in future with better way
 *
 * Finalize information which is necessary to convert
 *  only contains in get_block API but not get_transaction API
 */
pub async fn convert_txn_to_confirmed_txn<N: Network>(
    transaction: Transaction<N>,
) -> AvailResult<ConfirmedTransaction<N>> {
    let client = LocalClient::new(
        env!("TESTNET_API_OBSCURA").to_string(),
        "https://aleo-testnet3.obscura.build".to_string(),
        "testnet3".to_string(),
    );

    let transaction_id = transaction.id();
    let block = client
        .get_block_from_transaction_id::<N>(&transaction_id.to_string())
        .await?;

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
async fn convert_to_sync_txn_params<N: Network>(
    transitions: Vec<String>,
    client: &LocalClient,
) -> AvailResult<Vec<SyncTxnParams<N>>> {
    let mut sync_txn_params: Vec<SyncTxnParams<N>> = Vec::new();

    for transition in transitions {
        // Get block
        let transaction_id = client
            .get_transaction_id_from_transition(&transition)
            .await?;
        let block: Block<N> = client
            .get_block_from_transaction_id(&transaction_id)
            .await?;

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
pub async fn get_records_new<N: Network>(start: u32, end: u32) -> AvailResult<(Vec<Value>)> {
    // Prepare API client and get records
    // ATTENTION: Different API and base_url to get records
    let api_key: String = env!("OBSCURA_SDK").to_string();
    let client = LocalClient::new(
        api_key,
        "https://aleo-testnet3.dev.obscura.build".to_string(),
        "testnet3".to_string(),
    );
    let records = client.get_records(start, end).await?;
    Ok(records)
}

/**
 * Get sync transaction parameters.
 * @param records The records to get the sync transaction parameters from.
 * @return The sync transaction parameters.
 */
pub async fn get_sync_txn_params<N: Network>(
    records: Vec<Value>,
    window: Option<Window>,
) -> AvailResult<Vec<SyncTxnParams<N>>> {
    let timer1 = std::time::Instant::now();
    // Get transitions from owned records
    let transitions = owned_records_to_transitions::<N>(records, window)?;

    println!("Owned records to transitions took: {:?}", timer1.elapsed());

    // Prepare API client
    let api_key = env!("TESTNET_API_OBSCURA").to_string();
    let client = LocalClient::new(
        api_key,
        "https://aleo-testnet3.obscura.build".to_string(),
        "testnet3".to_string(),
    );

    let timer2 = std::time::Instant::now();

    // Get sync transaction parameters
    let sync_txn_params = convert_to_sync_txn_params(transitions, &client).await?;

    println!(
        "Convert to sync transaction parameters took: {:?}",
        timer2.elapsed()
    );

    Ok(sync_txn_params)
}

/**
 * Convert pending transactions to failed if they have expired.
 * Convert pending transactions to confirmed if they have been confirmed.
 * Convert pending transactions to rejected if they have been rejected.
 * Convert failed transactions to confirmed if they have been confirmed.
 * Convert failed transactions to rejected if they have been rejected.
 */
pub async fn handle_unconfirmed_transactions<N: Network>() -> AvailResult<()> {
    let view_key = VIEWSESSION.get_instance::<N>()?;
    let address = view_key.to_address();

    // checks if unconfirmed transactions have expired and updates their state to failed
    check_unconfirmed_transactions::<N>()?;

    // Get all unconfirmed and failed stored pointers
    let unconfirmed_and_failed_ids = get_unconfirmed_and_failed_transaction_ids::<N>()?;

    for (tx_id, pointer_id) in unconfirmed_and_failed_ids {
        let block = get_block_from_transaction_id::<N>(&tx_id.to_string()).await?;
        let height = block.height();

        let timestamp = get_timestamp_from_i64(block.timestamp())?;

        for transaction in block.transactions().iter() {
            let inner_tx = transaction.transaction();
            let fee = match inner_tx.fee_amount() {
                Ok(fee) => *fee as f64 / 1000000.0,
                Err(_) => {
                    handle_block_scan_failure::<N>(height)?;

                    return Err(AvailError::new(
                        AvailErrorType::SnarkVm,
                        "Error calculating fee".to_string(),
                        "Issue calculating fee".to_string(),
                    ));
                }
            };

            if let ConfirmedTransaction::<N>::AcceptedExecute(_, _, _) = transaction {
                let executed_transitions = match get_executed_transitions::<N>(inner_tx, height) {
                    Ok(transitions) => transitions,
                    Err(e) => {
                        handle_block_scan_failure::<N>(height)?;

                        return Err(AvailError::new(
                            AvailErrorType::SnarkVm,
                            e.to_string(),
                            "Error getting executed transitions".to_string(),
                        ));
                    }
                };

                match handle_transaction_confirmed(
                    pointer_id.as_str(),
                    tx_id,
                    executed_transitions,
                    height,
                    timestamp,
                    Some(fee),
                    address,
                ) {
                    Ok(_) => {}
                    Err(e) => {
                        handle_block_scan_failure::<N>(height)?;

                        return Err(AvailError::new(
                            AvailErrorType::Internal,
                            e.to_string(),
                            "Error handling confirmed transaction".to_string(),
                        ));
                    }
                };

                continue;
            } else if let ConfirmedTransaction::<N>::AcceptedDeploy(_, _, _) = transaction {
                if let Some(fee_transition) = transaction.fee_transition() {
                    let transition = fee_transition.transition();

                    match input_spent_check(transition, true) {
                        Ok(_) => {}
                        Err(e) => {
                            handle_block_scan_failure::<N>(height)?;

                            return Err(AvailError::new(
                                AvailErrorType::Internal,
                                e.to_string(),
                                "Error checking spent input".to_string(),
                            ));
                        }
                    };

                    match transition_to_record_pointer(tx_id, transition.clone(), height, view_key)
                    {
                        Ok(_) => {}
                        Err(e) => {
                            handle_block_scan_failure::<N>(height)?;

                            return Err(AvailError::new(
                                AvailErrorType::Internal,
                                e.to_string(),
                                "Error finding records from transition".to_string(),
                            ));
                        }
                    };
                }

                match handle_deployment_confirmed(
                    pointer_id.as_str(),
                    tx_id,
                    height,
                    Some(fee),
                    address,
                ) {
                    Ok(_) => {}
                    Err(e) => {
                        handle_block_scan_failure::<N>(height)?;

                        return Err(AvailError::new(
                            AvailErrorType::Internal,
                            e.to_string(),
                            "Error handling confirmed deployment".to_string(),
                        ));
                    }
                };

                continue;
            } else if let ConfirmedTransaction::<N>::RejectedDeploy(_, fee_tx, _, _) = transaction {
                let deployment_pointer = match get_deployment_pointer::<N>(pointer_id.as_str()) {
                    Ok(pointer) => pointer,
                    Err(e) => {
                        handle_block_scan_failure::<N>(height)?;

                        return Err(AvailError::new(
                            AvailErrorType::Internal,
                            e.to_string(),
                            "Error getting deployment pointer".to_string(),
                        ));
                    }
                };

                if let Some(fee_transition) = fee_tx.fee_transition() {
                    let transition = fee_transition.transition();

                    match input_spent_check(transition, true) {
                        Ok(_) => {}
                        Err(e) => {
                            handle_block_scan_failure::<N>(height)?;

                            return Err(AvailError::new(
                                AvailErrorType::Internal,
                                e.to_string(),
                                "Error checking spent input".to_string(),
                            ));
                        }
                    };

                    match transition_to_record_pointer(tx_id, transition.clone(), height, view_key)
                    {
                        Ok(_) => {}
                        Err(e) => {
                            handle_block_scan_failure::<N>(height)?;

                            return Err(AvailError::new(
                                AvailErrorType::Internal,
                                e.to_string(),
                                "Error finding records from transition".to_string(),
                            ));
                        }
                    };
                }

                match handle_deployment_rejection(
                    deployment_pointer,
                    pointer_id.as_str(),
                    tx_id,
                    height,
                    Some(fee),
                    address,
                ) {
                    Ok(_) => {}
                    Err(e) => {
                        handle_block_scan_failure::<N>(height)?;

                        return Err(AvailError::new(
                            AvailErrorType::Internal,
                            e.to_string(),
                            "Error handling rejected deployment".to_string(),
                        ));
                    }
                };

                continue;
            } else if let ConfirmedTransaction::<N>::RejectedExecute(_, fee_tx, rejected_tx, _) =
                transaction
            {
                let transaction_pointer = match get_transaction_pointer::<N>(pointer_id.as_str()) {
                    Ok(pointer) => pointer,
                    Err(e) => {
                        handle_block_scan_failure::<N>(height)?;

                        return Err(AvailError::new(
                            AvailErrorType::Internal,
                            e.to_string(),
                            "Error getting transaction pointer".to_string(),
                        ));
                    }
                };

                if let Some(fee_transition) = fee_tx.fee_transition() {
                    let transition = fee_transition.transition();

                    match input_spent_check(transition, true) {
                        Ok(_) => {}
                        Err(e) => {
                            handle_block_scan_failure::<N>(height)?;

                            return Err(AvailError::new(
                                AvailErrorType::Internal,
                                e.to_string(),
                                "Error checking spent input".to_string(),
                            ));
                        }
                    };

                    match transition_to_record_pointer(tx_id, transition.clone(), height, view_key)
                    {
                        Ok(_) => {}
                        Err(e) => {
                            handle_block_scan_failure::<N>(height)?;

                            return Err(AvailError::new(
                                AvailErrorType::Internal,
                                e.to_string(),
                                "Error finding records from transition".to_string(),
                            ));
                        }
                    };
                }

                if let Some(rejected_execution) = rejected_tx.execution() {
                    match handle_transaction_rejection(
                        transaction_pointer,
                        pointer_id.as_str(),
                        Some(rejected_execution.clone()),
                        Some(tx_id),
                        height,
                        Some(fee),
                        address,
                    ) {
                        Ok(_) => {}
                        Err(e) => {
                            handle_block_scan_failure::<N>(height)?;

                            return Err(AvailError::new(
                                AvailErrorType::Internal,
                                e.to_string(),
                                "Error handling rejected transaction".to_string(),
                            ));
                        }
                    };

                    continue;
                }

                match handle_transaction_rejection(
                    transaction_pointer,
                    pointer_id.as_str(),
                    None,
                    Some(tx_id),
                    height,
                    Some(fee),
                    address,
                ) {
                    Ok(_) => {}
                    Err(e) => {
                        handle_block_scan_failure::<N>(height)?;

                        return Err(AvailError::new(
                            AvailErrorType::Internal,
                            e.to_string(),
                            "Error handling rejected transaction".to_string(),
                        ));
                    }
                };

                continue;
            }
            continue;
        }
    }

    Ok(())
}

pub async fn public_scanning<N: Network>(
    address: &str,
    start: u32,
    end: u32,
    limit: u32
) -> AvailResult<Vec<TransitionPointer<N>>> {
    // Setup API and get public transitions
    // ATTENTION: Different API and base_url to get records
    let api_key: String = env!("OBSCURA_SDK").to_string();
    let client = LocalClient::new(
        api_key,
        "https://aleo-testnet3.dev.obscura.build".to_string(),
        "testnet3".to_string(),
    );

    // Get public transitions
    let transactions_value = client.get_pub_tsn_by_addr_and_height(address, start, end, limit).await?;
    println!("Transactions Value: \n{:?}\n", transactions_value);

    let mut transition_pointers: Vec<TransitionPointer<N>> = Vec::new();
    for transaction in transactions_value {
        let transition_id_str = transaction.get("transition_id").unwrap().as_str().unwrap();

        // Skip if already stored in local storage
        if is_transition_stored(transition_id_str)? {
            continue;
        }

        let transition_id = match N::TransitionID::from_str(transition_id_str) {
            Ok(id) => id,
            Err(_) => return Err(AvailError::new(
                AvailErrorType::Internal,
                "Failed to parse transition ID".to_string(),
                "Failed to parse transition ID".to_string(),
            )),
        };
        let transaction_id = match N::TransactionID::from_str(transaction.get("transaction_id").unwrap().as_str().unwrap()) {
            Ok(id) => id,
            Err(_) => return Err(AvailError::new(
                AvailErrorType::Internal,
                "Failed to parse transaction ID".to_string(),
                "Failed to parse transaction ID".to_string(),
            )),
        };

        // Build transition pointer
        let transition_pointer: TransitionPointer<N> = TransitionPointer::new(
            transition_id,
            transaction_id,
            transaction.get("transition").unwrap().get("program").unwrap().to_string(),
            "function_id".to_string(),
            DateTime::from_timestamp(transaction.get("timestamp").unwrap().as_i64().unwrap(), 0)
                .unwrap()
                .with_timezone(&Local),
            TransitionType::Output,
            None,
            None,
            None,
            transaction.get("block_height").unwrap().as_u64().unwrap() as u32,
        );

        println!("Transition Pointer: \n{:?}\n", transition_pointer);

        // Store in local storage
        let (_, address): (_, Address<N>) = Address::parse(address).unwrap();
        let encrypted_transition_pointer = transition_pointer.to_encrypted_data(address)?;
        store_encrypted_data(encrypted_transition_pointer.clone())?;

        transition_pointers.push(transition_pointer);
    }

    Ok(transition_pointers)
}

#[cfg(test)]
mod record_handling_tests {
    use super::*;
    use crate::models::pointers::record::AvailRecord;
    use avail_common::models::encrypted_data::EncryptedData;

    #[tokio::test]
    async fn test_public_scanning() {
        type N = Testnet3;

        let view_key = env!("VIEW_KEY");
        VIEWSESSION.set_view_session(view_key).unwrap();

        let view_key = VIEWSESSION.get_instance::<N>().unwrap();
        let address = view_key.to_address();
        let start: u32 = 0;;
        let end: u32 = 2448210;
        let limit: u32 = 100;
        let transactions = public_scanning::<N>(
            &address.to_string(),
            start,
            end,
            limit
        ).await;

        // Check result status
        match transactions {
            Ok(_) => println!("Public Scanning Successful\n"),
            Err(ref e) => println!("Public Scanning Failed\n {}\n", e),
        }
        assert!(transactions.is_ok());
    }

    #[tokio::test]
    async fn test_get_records() {
        type N = Testnet3;

        let view_key = env!("VIEW_KEY");
        VIEWSESSION.set_view_session(view_key).unwrap();
        let current_block: u32 = 2087986;
        let start: u32 = current_block - 100;
        let records = get_records_new::<N>(start, current_block).await;

        assert!(records.is_ok());
    }

    #[tokio::test]
    async fn test_get_sync_txn_params() {
        type N = Testnet3;

        let view_key = env!("VIEW_KEY");
        VIEWSESSION.set_view_session(view_key).unwrap();
        let current_block: u32 = 2087986;
        let start: u32 = current_block - 10;
        let records = get_records_new::<N>(start, current_block).await.unwrap();

        let sync_txn_params = get_sync_txn_params::<N>(records, None).await;

        assert!(sync_txn_params.is_ok());
    }

    #[tokio::test]
    async fn test_sync_transaction() {
        type N = Testnet3;

        let view_key = env!("VIEW_KEY");
        VIEWSESSION.set_view_session(view_key).unwrap();
        let current_block: u32 = 2249244;
        let start: u32 = current_block - 10;
        let records = get_records_new::<N>(start, current_block).await.unwrap();

        // TODO : Check if the record is already stored

        let sync_txn_params = get_sync_txn_params::<N>(records, None).await.unwrap();

        let mut res: bool = false;

        // Sync transactions
        for params in sync_txn_params {
            let (_, _, found) = sync_transaction::<Testnet3>(
                &params.transaction,
                params.block_height,
                params.timestamp,
                None,
                None,
            )
            .unwrap();

            if !res {
                res = found;
            }
        }
        println!("Result of sync_transaction:\n{:?}\n", res);
        assert!(res);
    }
}
