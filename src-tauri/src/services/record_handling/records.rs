use chrono::Local;
use futures::lock::MutexGuard;
use snarkvm::{
    circuit::group::add,
    console::program::Itertools,
    ledger::block::*,
    prelude::{ConfirmedTransaction, Network, Plaintext, Record},
};
use std::ops::Sub;
use tauri::{Manager, Window};

use crate::{
    api::{
        aleo_client::{setup_aleo_client, setup_client, setup_local_client},
        backup_recovery::update_sync_height,
    },
    helpers::utils::get_timestamp_from_i64,
    models::wallet_connect::records::{GetRecordsRequest, RecordFilterType, RecordsFilter},
    services::{
        local_storage::{
            encrypted_data::{
                handle_block_scan_failure, update_encrypted_transaction_confirmed_by_id,
                update_encrypted_transaction_state_by_id,
            },
            persistent_storage::{get_address_string, update_last_sync},
            session::view::VIEWSESSION,
            storage_api::{
                deployment::{find_encrypt_store_deployments, get_deployment_pointer},
                records::{get_record_pointers, get_record_pointers_for_record_type},
                transaction::{
                    check_unconfirmed_transactions, get_transaction_pointer, get_tx_ids_from_date,
                    get_unconfirmed_and_failed_transaction_ids,
                },
            },
        },
        record_handling::utils::{
            get_executed_transitions, handle_deployment_confirmed, handle_deployment_rejection,
            handle_transaction_confirmed, handle_transaction_rejection, input_spent_check,
            sync_transaction, transition_to_record_pointer,
        },
    },
};
use rayon::prelude::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tauri::Emitter;

use avail_common::{
    aleo_tools::program_manager::Credits,
    errors::{AvailError, AvailErrorType, AvailResult},
    models::encrypted_data::{EncryptedData, RecordTypeCommon, TransactionState},
};

/// Scans the blockchain for new records, distills record pointers, transition pointer and tags, and returns them
pub fn get_records<N: Network>(
    last_sync: u32,
    height: u32,
    window: Option<Window>,
) -> AvailResult<bool> {
    let view_key = VIEWSESSION.get_instance::<N>()?;
    let address = view_key.to_address();

    let mut api_client = setup_client::<N>()?;

    let step_size = 49;

    let amount_to_scan = height.sub(last_sync);
    let latest_height = height;

    let (last_sync_block, mut api_client) = match api_client.get_block(last_sync) {
        Ok(block) => (block, api_client.clone()),
        Err(e) => {
            if e.to_string().contains("Invalid Circuit")
                || e.to_string().contains("Invalid Data")
                || e.to_string().contains("Failed to parse block")
                || e.to_string().contains("JSON")
            {
                let api_client_aleo = setup_aleo_client::<N>()?;
                println!(
                    "Obscura API endpoint is failing, switching to aleo API - {}",
                    api_client_aleo.base_url()
                );
                let block = api_client_aleo.get_block(last_sync)?;
                (block, api_client_aleo)
            } else {
                return Err(AvailError::new(
                    AvailErrorType::Internal,
                    e.to_string(),
                    "Error getting block".to_string(),
                ));
            }
        }
    };
    let last_sync_timestamp = get_timestamp_from_i64(last_sync_block.timestamp())?;

    // checks if unconfirmed transactions have expired and updates their state to failed
    check_unconfirmed_transactions::<N>()?;

    let stored_transaction_ids = get_tx_ids_from_date::<N>(last_sync_timestamp)?;

    println!("Stored transaction ids: {:?}", stored_transaction_ids);

    let unconfirmed_and_failed_ids = get_unconfirmed_and_failed_transaction_ids::<N>()?;

    println!(
        "Unconfirmed and failed ids: {:?}",
        unconfirmed_and_failed_ids
    );

    let unconfirmed_and_failed_transaction_ids = unconfirmed_and_failed_ids
        .iter()
        .map(|(id, _)| *id)
        .collect::<Vec<N::TransactionID>>();

    let stored_transaction_ids = stored_transaction_ids
        .iter()
        .filter(|id| !unconfirmed_and_failed_transaction_ids.contains(id))
        .cloned()
        .collect_vec();

    println!(
        "Stored transaction ids without unconfirmed and failed: {:?}",
        stored_transaction_ids
    );

    let mut end_height = last_sync.saturating_add(step_size);
    let mut start_height = last_sync;

    if end_height > latest_height {
        end_height = latest_height;
    }

    let mut found_flag = false;
    println!("API Client{:?}", api_client.base_url());

    for _ in (last_sync..latest_height).step_by(step_size as usize) {
        let mut blocks = match api_client.get_blocks(start_height, end_height) {
            Ok(blocks) => blocks,
            Err(e) => {
                println!("Error getting blocks: {:?}", e.to_string());

                if &e.to_string() == "zero txs error" {
                    start_height = end_height;
                    end_height = start_height.saturating_add(step_size);
                    if end_height > latest_height {
                        end_height = latest_height;
                    };

                    continue;
                }

                if e.to_string().contains("500")
                    || e.to_string().contains("504")
                    || e.to_string().contains("status code 500")
                    || e.to_string().contains("Error getting blocks")
                    || e.to_string().contains("https://aleo-testnetbeta.obscura.network/v1/92acf30f-5cea-4679-880c-f06e9a7e8465/testnet/blocks?start=")
                    || e.to_string().contains("Invalid Data")
                    || e.to_string().contains("Failed to parse block")
                    || e.to_string().contains("JSON")
                {
                    api_client = setup_aleo_client::<N>()?;
                    println!("Switched to aleo client;;;;{:?}", api_client.base_url());
                    continue;
                } else {
                    return Err(AvailError::new(
                        AvailErrorType::Internal,
                        e.to_string(),
                        "Error getting blocks".to_string(),
                    ));
                };

                return Err(AvailError::new(
                    AvailErrorType::Internal,
                    e.to_string(),
                    "Error getting blocks".to_string(),
                ));
            }
        };

        for block in blocks {
            // Check for deployment transactions
            let transactions = block.transactions();
            let timestamp = get_timestamp_from_i64(block.clone().timestamp())?;
            let height = block.height();

            match find_encrypt_store_deployments(
                transactions,
                height,
                timestamp,
                address,
                stored_transaction_ids.clone(),
            ) {
                Ok(_) => {}
                Err(e) => {
                    handle_block_scan_failure::<N>(height)?;

                    return Err(AvailError::new(
                        AvailErrorType::Internal,
                        e.to_string(),
                        "Error scanning deployment transactions.".to_string(),
                    ));
                }
            }

            for transaction in transactions.iter() {
                let transaction_id = transaction.id();

                let unconfirmed_transaction_id = match transaction.to_unconfirmed_transaction_id() {
                    Ok(id) => id,
                    Err(_) => {
                        handle_block_scan_failure::<N>(height)?;

                        return Err(AvailError::new(
                            AvailErrorType::SnarkVm,
                            "Error getting unconfirmed transaction id".to_string(),
                            "Issue getting unconfirmed transaction id".to_string(),
                        ));
                    }
                };

                if stored_transaction_ids.contains(&transaction_id)
                    || stored_transaction_ids.contains(&unconfirmed_transaction_id)
                {
                    continue;
                }

                if let Some((tx_id, pointer_id)) =
                    unconfirmed_and_failed_ids.iter().find(|(tx_id, _)| {
                        tx_id == &transaction_id || tx_id == &unconfirmed_transaction_id
                    })
                {
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
                        let executed_transitions =
                            match get_executed_transitions::<N>(inner_tx, height) {
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
                            *tx_id,
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

                            match transition_to_record_pointer(
                                *tx_id,
                                transition.clone(),
                                height,
                                view_key,
                            ) {
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
                            *tx_id,
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
                    } else if let ConfirmedTransaction::<N>::RejectedDeploy(_, fee_tx, _, _) =
                        transaction
                    {
                        let deployment_pointer =
                            match get_deployment_pointer::<N>(pointer_id.as_str()) {
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

                            match transition_to_record_pointer(
                                *tx_id,
                                transition.clone(),
                                height,
                                view_key,
                            ) {
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
                            *tx_id,
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
                    } else if let ConfirmedTransaction::<N>::RejectedExecute(
                        _,
                        fee_tx,
                        rejected_tx,
                        _,
                    ) = transaction
                    {
                        let transaction_pointer =
                            match get_transaction_pointer::<N>(pointer_id.as_str()) {
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

                            match transition_to_record_pointer(
                                *tx_id,
                                transition.clone(),
                                height,
                                view_key,
                            ) {
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
                                Some(*tx_id),
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
                            Some(*tx_id),
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

                let (_, _, _, bool_flag) =
                    match sync_transaction::<N>(transaction, height, timestamp, None, None) {
                        Ok(transaction_result) => transaction_result,

                        Err(e) => {
                            match handle_block_scan_failure::<N>(height) {
                                Ok(_) => {}
                                Err(e) => {
                                    return Err(AvailError::new(
                                        AvailErrorType::Internal,
                                        e.to_string(),
                                        "Error syncing transaction".to_string(),
                                    ));
                                }
                            }

                            return Err(AvailError::new(
                                AvailErrorType::Internal,
                                e.to_string(),
                                "Error syncing transaction".to_string(),
                            ));
                        }
                    };

                if !found_flag {
                    found_flag = bool_flag;
                }
            }

            match update_last_sync(height) {
                Ok(_) => {
                    println!("Synced {}", height);
                }
                Err(e) => {
                    match handle_block_scan_failure::<N>(height) {
                        Ok(_) => {}
                        Err(e) => {
                            return Err(AvailError::new(
                                AvailErrorType::Internal,
                                e.to_string(),
                                "Error syncing transaction".to_string(),
                            ));
                        }
                    }

                    return Err(AvailError::new(
                        AvailErrorType::Internal,
                        e.to_string(),
                        "Error updating last synced block height".to_string(),
                    ));
                }
            };

            let percentage =
                (((height - last_sync) as f32 / amount_to_scan as f32) * 10000.0).round() / 100.0;

            let percentage = if percentage > 100.0 {
                100.0
            } else {
                percentage
            };

            // update progress bar
            if let Some(window) = window.clone() {
                match window.emit("scan_progress", percentage) {
                    Ok(_) => {}
                    Err(e) => {
                        match handle_block_scan_failure::<N>(height) {
                            Ok(_) => {}
                            Err(e) => {
                                return Err(AvailError::new(
                                    AvailErrorType::Internal,
                                    e.to_string(),
                                    "Error syncing transaction".to_string(),
                                ));
                            }
                        }

                        return Err(AvailError::new(
                            AvailErrorType::Internal,
                            e.to_string(),
                            "Error updating progress bar".to_string(),
                        ));
                    }
                };
            }
        }

        // Search in reverse order from the latest block to the earliest block
        start_height = end_height;
        end_height = start_height.saturating_add(step_size);
        if end_height > latest_height {
            end_height = latest_height;
        };
    }

    Ok(found_flag)
}

/// Fetches an aleo credits record to spend
pub fn find_aleo_credits_record_to_spend<N: Network>(
    amount: &u64,
    previous: Vec<String>,
) -> AvailResult<(Record<N, Plaintext<N>>, String, String)> {
    let address = get_address_string()?;
    let (record_pointers, encrypted_record_ids) =
        get_record_pointers_for_record_type::<N>(RecordTypeCommon::AleoCredits, &address)?;

    let mut iter = 0;
    let mut balance_counter = 0u64;

    for record in record_pointers.iter() {
        if record.metadata.spent {
            iter += 1;
            continue;
        }
        if previous.clone().contains(&record.metadata.nonce) {
            iter += 1;
            continue;
        }

        let aleo_record = record.to_record()?;
        let record_amount = aleo_record.microcredits()?;

        if &record_amount >= amount {
            return Ok((
                aleo_record,
                record.pointer.commitment.clone(),
                encrypted_record_ids[iter].clone(),
            ));
        }

        iter += 1;
        balance_counter += record_amount;
    }

    // TODO - implement join_n
    if &balance_counter > amount {
        return Err(AvailError::new(
            AvailErrorType::Internal,
            "Join aleo credit records to obtain a sufficient balance.".to_string(),
            "Join aleo credit records to obtain a sufficient balance.".to_string(),
        ));
    }

    Err(AvailError::new(
        AvailErrorType::Internal,
        "Not enough balance".to_string(),
        "Not enough balance".to_string(),
    ))

    // find first record that satisfies the amount required
}

pub fn find_tokens_to_spend<N: Network>(
    asset_id: &str,
    amount: &u64,
    previous: Vec<String>,
) -> AvailResult<(Record<N, Plaintext<N>>, String, String)> {
    let _address = get_address_string()?;
    let program_id = format!("{}{}", asset_id, ".aleo");
    let record_name = format!("{}{}", asset_id, ".record");

    let filter = RecordsFilter::new(
        vec![program_id.to_string()],
        None,
        RecordFilterType::Unspent,
        Some(record_name.to_string()),
    );
    let get_records_request = GetRecordsRequest::new(None, Some(filter), None);
    let (record_pointers, ids) = get_record_pointers::<N>(get_records_request)?;

    let mut iter = 0;
    let mut balance_counter = 0u64;

    for record in record_pointers.iter() {
        if record.metadata.spent {
            iter += 1;
            continue;
        }
        if previous.clone().contains(&record.metadata.nonce) {
            iter += 1;
            continue;
        }

        let aleo_record = record.to_record()?;
        let record_amount = aleo_record.microcredits()?;

        if &record_amount >= amount {
            return Ok((
                aleo_record,
                record.pointer.commitment.clone(),
                ids[iter].clone(),
            ));
        }

        iter += 1;
        balance_counter += record_amount;
    }

    // TODO - implement join_n
    if &balance_counter > amount {
        return Err(AvailError::new(
            AvailErrorType::Internal,
            "Join token records to obtain a sufficient balance.".to_string(),
            "Join token records to obtain a sufficient balance.".to_string(),
        ));
    }

    Err(AvailError::new(
        AvailErrorType::Internal,
        "Not enough balance".to_string(),
        "Not enough balance".to_string(),
    ))

    // find first record that satisfies the amount required
}

///Joins two records together
/// TODO - Join n records to meet amount x
/*
async fn join_records<N: Network>(
    pk: PrivateKey<N>,
    amount: u64,
    token: &str,
) -> AvailResult<String> {
    let fee = 10000u64;

    let fee_record = find_aleo_credits_record_to_spend::<N>(fee, vec![])?;

    // TODO - iteratively find records until amount is satisfied


    let inputs: Vec<Value<N>> = vec![Value::Record(input_record), Value::Record(input2_record)];

    let api_client = AleoAPIClient::<N>::local_testnet3("3030");
    let mut program_manager =
        ProgramManager::<N>::new(Some(pk), None, Some(api_client), None).unwrap();

    //calculate estimate

    let join_execution = program_manager.execute_program(
        "credits.aleo",
        "join",
        inputs.iter(),
        fee,
        fee_record,
        None,
    )?;

    update_identifier_status(fee_commitment, &fee_id).await?;
    update_identifier_status(input_commitment, &input_id).await?;
    update_identifier_status(input2_commitment, &input2_id).await?;

    //check tx block, normal post tx procedure
    Ok(join_execution)
}
*/

///Splits a record into two records
/*
async fn split_records<N: Network>(
    pk: PrivateKey<N>,
    amount: u64,
    token: &str,
) -> AvailResult<String> {
    let fee = 10000u64;

    let fee_record = find_aleo_credits_record_to_spend::<N>(fee, vec![])?;

    let input_record = find_aleo_credits_record_to_spend::<N>(amount, vec![])?;

    let inputs: Vec<Value<N>> = vec![Value::Record(input_record)];

    let api_client = AleoAPIClient::<N>::local_testnet3("3030");
    let mut program_manager =
        ProgramManager::<N>::new(Some(pk), None, Some(api_client), None).unwrap();

    let split_execution = program_manager.execute_program(
        "credits.aleo",
        "split",
        inputs.iter(),
        fee,
        fee_record,
        None,
    )?;

    //TODO - How to get commitment from record

    update_identifier_status(fee_record.to_commitment(program_id, record_name), &fee_id).await?;
    update_identifier_status(input_commitment, &input_id).await?;

    Ok(split_execution)
}
*/

#[cfg(test)]
mod record_handling_test {
    use super::*;
    use crate::{
        api::aleo_client::setup_client, services::local_storage::persistent_storage::get_last_sync,
    };
    use snarkvm::prelude::{AleoID, Field, TestnetV0};
    use std::str::FromStr;

    #[test]
    fn test_get_transaction() {
        let start = 500527u32;
        let end = 500531u32;

        let api_client = setup_client::<TestnetV0>().unwrap();

        let blocks = api_client.get_blocks(start, end).unwrap();

        let tx_id = &AleoID::<Field<TestnetV0>, 29793>::from_str(
            "at1w8t8pkc9xuf2p05gp9fanxpx0h53jmpguc07ja34s3jm905v65gss306rr",
        );

        for block in blocks {
            let transactions = block.transactions();

            match tx_id {
                Ok(tx_id) => {
                    let tx = transactions.get(tx_id);
                    let info = match tx {
                        Some(tx) => tx,
                        None => {
                            println!("tx not found");
                            continue;
                        }
                    };
                    println!("info: {:?}", info);
                }
                Err(e) => {
                    print!("{}", e.to_string())
                }
            }
        }
    }

    /*
    #[test]
    fn test_nova() {
        let _res = get_nova_records::<TestnetV0>(372243).unwrap();

        println!("res: {:?}", _res);
    }
    */

    #[test]
    fn test_get_records() {
        let api_client = setup_client::<TestnetV0>().unwrap();

        let latest_height = api_client.latest_height().unwrap();
        let last_sync = get_last_sync().unwrap();

        let _res = get_records::<TestnetV0>(last_sync, latest_height, None).unwrap();

        println!("res: {:?}", _res);
    }

    #[test]
    fn find_aleo_credits_record_to_spend_test() {
        let _res = find_aleo_credits_record_to_spend::<TestnetV0>(&10000, vec![]).unwrap();

        println!("res: {:?}", _res);
    }

    /*
        #[tokio::test]
        async fn handle_unconfirmed_transactions_test() {
            VIEWSESSION
            .set_view_session("AViewKey1h4qXQ8kP2JT7Vo7pBuhtMrHz7R81RJUHLc2LTQfrCt3R")
            .unwrap();

           handle_unconfirmed_transactions::<TestnetV0>().await.unwrap();
        }
    */

    #[tokio::test]
    async fn test_good_block_height() {
        let api_client = setup_client::<TestnetV0>().unwrap();
        let latest_height = api_client.latest_height().unwrap();
        let mut last_sync = 0u32;
        let mut flag = true;
        while flag {
            let last_sync_block = match api_client.get_block(last_sync) {
                Ok(block) => {
                    println!("Block: {:?}", block);
                    flag = false;
                    block
                }
                Err(e) => {
                    println!("Error getting block: {:?}", e.to_string());
                    last_sync += 1;
                    continue;
                }
            };
        }
    }
}
