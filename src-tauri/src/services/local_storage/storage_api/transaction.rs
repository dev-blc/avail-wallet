use chrono::{DateTime, Local, Utc};
use snarkvm::prelude::Network;

use crate::models::{
    event::Event,
    pointers::{
        deployment::DeploymentPointer, transaction::TransactionPointer,
        transition::TransitionPointer,
    },
};
use crate::services::local_storage::encrypted_data::{
    get_encrypted_data_by_id, get_encrypted_data_by_transaction_id, handle_encrypted_data_query,
    handle_encrypted_data_query_params, update_encrypted_transaction_state_by_id,
};
use crate::services::local_storage::{
    encrypted_data::get_encrypted_data_by_flavour,
    persistent_storage::{get_address, get_network},
    session::view::VIEWSESSION,
};

use avail_common::{
    errors::{AvailError, AvailErrorType, AvailResult},
    models::encrypted_data::{EncryptedData, EncryptedDataTypeCommon, TransactionState},
};

use super::{deployment::get_deployment_pointer, records::update_record_spent_local_via_nonce};

pub fn get_transactions_exec<N: Network>() -> AvailResult<Vec<TransactionPointer<N>>> {
    let encrypted_transactions =
        get_encrypted_data_by_flavour(EncryptedDataTypeCommon::Transaction)?;

    let transactions = decrypt_transactions_exec(encrypted_transactions)?;

    Ok(transactions)
}

pub fn get_transaction_pointer<N: Network>(id: &str) -> AvailResult<TransactionPointer<N>> {
    let encrypted_transaction = get_encrypted_data_by_id(id)?;

    let transaction = decrypt_transactions_exec::<N>(vec![encrypted_transaction])?;

    Ok(transaction[0].to_owned())
}

pub fn decrypt_transactions_exec<N: Network>(
    encrypted_transactions: Vec<EncryptedData>,
) -> AvailResult<Vec<TransactionPointer<N>>> {
    let v_key = VIEWSESSION.get_instance::<N>()?;

    let transactions = encrypted_transactions
        .iter()
        .map(|x| {
            let encrypted_data = x.to_enrypted_struct::<N>()?;

            let tx_in: TransactionPointer<N> = encrypted_data.decrypt(v_key)?;

            Ok(tx_in)
        })
        .collect::<AvailResult<Vec<TransactionPointer<N>>>>()?;

    Ok(transactions)
}

pub fn decrypt_transaction_exec<N: Network>(encrypted_tx: EncryptedData) -> AvailResult<Event> {
    let v_key = VIEWSESSION.get_instance::<N>()?;

    let encrypted_data = encrypted_tx.to_enrypted_struct::<N>()?;

    let tx_out: TransactionPointer<N> = encrypted_data.decrypt(v_key)?;

    let event_id = match encrypted_tx.id {
        Some(id) => id.to_string(),
        None => {
            return Err(AvailError::new(
                AvailErrorType::Internal,
                "No event id found".to_string(),
                "No event id found".to_string(),
            ))
        }
    };

    tx_out.to_event(&event_id)
}

/* Utilities */
// get all transactions from a certain synced_on date forward and return Vector of transaction id string
pub fn get_tx_ids_from_date<N: Network>(
    date: DateTime<Local>,
) -> AvailResult<Vec<N::TransactionID>> {
    let address = get_address::<N>()?;
    let network = get_network()?;

    // get a timestamp from 2 hours ago
    let timestamp = date.with_timezone(&Utc);
    let timestamp_2_hours_ago = timestamp - chrono::Duration::hours(2);

    let query = format!(
        "SELECT * FROM encrypted_data WHERE flavour IN ('{}','{}','{}') AND owner='{}' AND network='{}' AND created_at >= ?1",
        EncryptedDataTypeCommon::Transition.to_str(),
        EncryptedDataTypeCommon::Transaction.to_str(),
        EncryptedDataTypeCommon::Deployment.to_str(),
        address,
        network
    );

    let encrypted_transactions =
        handle_encrypted_data_query_params(&query, vec![timestamp_2_hours_ago])?;

    let mut transaction_ids: Vec<N::TransactionID> = Vec::new();

    for encrypted_transaction in encrypted_transactions {
        let encrypted_struct = encrypted_transaction.to_enrypted_struct::<N>()?;
        match encrypted_transaction.flavour {
            EncryptedDataTypeCommon::Transition => {
                let transition: TransitionPointer<N> =
                    encrypted_struct.decrypt(VIEWSESSION.get_instance::<N>()?)?;
                transaction_ids.push(transition.transaction_id);
            }
            EncryptedDataTypeCommon::Transaction => {
                let tx_exec: TransactionPointer<N> =
                    encrypted_struct.decrypt(VIEWSESSION.get_instance::<N>()?)?;
                if let Some(id) = tx_exec.transaction_id() {
                    transaction_ids.push(id);
                }
            }
            EncryptedDataTypeCommon::Deployment => {
                let deployment: DeploymentPointer<N> =
                    encrypted_struct.decrypt(VIEWSESSION.get_instance::<N>()?)?;
                if let Some(id) = deployment.id {
                    transaction_ids.push(id);
                }
            }
            _ => {}
        };
    }

    Ok(transaction_ids)
}

/*
pub fn check_txs_for_task<N:Network>(start_data: DateTime<Utc>, end_time: DateTime<Utc>, program_id: &str, function_id: &str) -> AvailResult<bool>{
    let address = get_address::<N>()?;
    let network = get_network()?;

    // I want to query the encrypted_data table for transactions that match the program_id and function_ud
}
*/

pub fn get_transaction_ids<N: Network>() -> AvailResult<Vec<N::TransactionID>> {
    let address = get_address::<N>()?;
    let network = get_network()?;

    let query = format!(
        "SELECT * FROM encrypted_data WHERE flavour IN ('{}','{}','{}') AND owner='{}' AND network='{}'",
        EncryptedDataTypeCommon::Transition.to_str(),
        EncryptedDataTypeCommon::Transaction.to_str(),
        EncryptedDataTypeCommon::Deployment.to_str(),
        address,
        network
    );

    let encrypted_transactions = handle_encrypted_data_query(&query)?;

    let mut transaction_ids: Vec<N::TransactionID> = Vec::new();

    for encrypted_transaction in encrypted_transactions {
        let encrypted_struct = encrypted_transaction.to_enrypted_struct::<N>()?;
        match encrypted_transaction.flavour {
            EncryptedDataTypeCommon::Transition => {
                let transition: TransitionPointer<N> =
                    encrypted_struct.decrypt(VIEWSESSION.get_instance::<N>()?)?;
                transaction_ids.push(transition.transaction_id);
            }
            EncryptedDataTypeCommon::Transaction => {
                let tx_exec: TransactionPointer<N> =
                    encrypted_struct.decrypt(VIEWSESSION.get_instance::<N>()?)?;

                if let Some(id) = tx_exec.transaction_id() {
                    transaction_ids.push(id);
                }
            }
            EncryptedDataTypeCommon::Deployment => {
                let deployment: DeploymentPointer<N> =
                    encrypted_struct.decrypt(VIEWSESSION.get_instance::<N>()?)?;
                if let Some(id) = deployment.id {
                    transaction_ids.push(id);
                }
            }
            _ => {}
        };
    }

    Ok(transaction_ids)
}

pub fn get_transaction_ids_for_quest_verification<N: Network>(
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    program_id: &str,
    function_id: &str,
) -> AvailResult<Vec<EncryptedData>> {
    // for quest verification we have to find encrypted_data where the flavour is Transaction,
    // the state is Confirmed, and the program_id and function_id match, and created_at is within the specified time range
    let address = get_address::<N>()?;
    let network = get_network()?;

    let start_time = start_time - chrono::Duration::hours(2);

    let time_filter = format!(
        "AND created_at BETWEEN '{}' AND '{}'",
        start_time.to_rfc3339(), // Converts DateTime<Utc> to a string in RFC3339 format suitable for SQL queries
        end_time.to_rfc3339()
    );

    // Query for transitions and deployments
    let transitions_deployments_query = format!(
        "SELECT *, '[]' as json_program_ids, '[]' as json_function_ids FROM encrypted_data WHERE flavour IN ('{}','{}') AND owner='{}' AND network='{}'",
        EncryptedDataTypeCommon::Transition.to_str(),
        EncryptedDataTypeCommon::Deployment.to_str(),
        address,
        network
    );

    // Query for transactions
    let transactions_query = format!(
        "SELECT *, program_ids as json_program_ids, function_ids as json_function_ids FROM encrypted_data WHERE flavour='{}' AND owner='{}' AND network='{}'",
        EncryptedDataTypeCommon::Transaction.to_str(),
        address,
        network
    );

    let mut common_filter_conditions = String::new();

    let program_id_filter = format!(
        "AND (program_ids='{}' OR JSON_EXTRACT(json_program_ids, '$') LIKE '%{}%')",
        program_id, program_id
    );

    let function_id_filter = format!(
        "AND (function_ids='{}' OR JSON_EXTRACT(json_function_ids, '$') LIKE '%{}%')",
        function_id, function_id
    );

    common_filter_conditions.push_str(&program_id_filter);
    common_filter_conditions.push_str(&function_id_filter);

    let transitions_deployments_query_with_filters = format!(
        "{} {}",
        transitions_deployments_query, common_filter_conditions
    );

    let transactions_query_with_filters =
        format!("{} {}", transactions_query, common_filter_conditions);

    let mut combined_query = format!(
        "{} UNION ALL {} ORDER BY created_at DESC",
        transitions_deployments_query_with_filters, transactions_query_with_filters
    );

    //let query = format!("{} {} {}", query, program_id_filter, function_id_filter);

    // Assuming the execution of the query and processing the result happens here

    let encrypted_transactions = handle_encrypted_data_query(&combined_query)?;
    Ok(encrypted_transactions)
}

pub fn get_unconfirmed_and_failed_transaction_ids<N: Network>(
) -> AvailResult<Vec<(N::TransactionID, String)>> {
    let address = get_address::<N>()?;
    let network = get_network()?;

    let query = format!(
        "SELECT * FROM encrypted_data WHERE flavour IN ('{}','{}','{}') AND state IN ('{}','{}') AND owner='{}' AND network='{}'",
        EncryptedDataTypeCommon::Transition.to_str(),
        EncryptedDataTypeCommon::Transaction.to_str(),
        EncryptedDataTypeCommon::Deployment.to_str(),
        TransactionState::Pending.to_str(),
        TransactionState::Failed.to_str(),
        address,
        network
    );

    let encrypted_transactions = handle_encrypted_data_query(&query)?;

    let mut transaction_ids: Vec<(N::TransactionID, String)> = Vec::new();

    for encrypted_transaction in encrypted_transactions {
        let encrypted_struct = encrypted_transaction.to_enrypted_struct::<N>()?;
        match encrypted_transaction.flavour {
            EncryptedDataTypeCommon::Transition => {
                let transition: TransitionPointer<N> =
                    encrypted_struct.decrypt(VIEWSESSION.get_instance::<N>()?)?;
                if let Some(id) = encrypted_transaction.id {
                    transaction_ids.push((transition.transaction_id, id.to_string()));
                }
            }
            EncryptedDataTypeCommon::Transaction => {
                let tx_exec: TransactionPointer<N> =
                    encrypted_struct.decrypt(VIEWSESSION.get_instance::<N>()?)?;

                if let Some(tx_id) = tx_exec.transaction_id() {
                    if let Some(id) = encrypted_transaction.id {
                        transaction_ids.push((tx_id, id.to_string()));
                    }
                }
            }
            EncryptedDataTypeCommon::Deployment => {
                let deployment: DeploymentPointer<N> =
                    encrypted_struct.decrypt(VIEWSESSION.get_instance::<N>()?)?;
                if let Some(tx_id) = deployment.id {
                    if let Some(id) = encrypted_transaction.id {
                        transaction_ids.push((tx_id, id.to_string()));
                    }
                }
            }
            _ => {}
        };
    }

    Ok(transaction_ids)
}

// gets unconfirmed transactions that have been unconfirmed for more than 10 minutes
fn get_expired_unconfirmed_transactions<N: Network>() -> AvailResult<Vec<EncryptedData>> {
    let address = get_address::<N>()?;
    let network = get_network()?;

    let query = format!(
        "SELECT * FROM encrypted_data WHERE flavour IN ('{}','{}','{}') AND state='{}' AND owner='{}' AND network='{}'",
        EncryptedDataTypeCommon::Transition.to_str(),
        EncryptedDataTypeCommon::Transaction.to_str(),
        EncryptedDataTypeCommon::Deployment.to_str(),
        TransactionState::Pending.to_str(),
        address,
        network
    );

    let now = Local::now();
    let encrypted_data = handle_encrypted_data_query(&query)?;
    let mut encrypted_transactions_to_decrypt: Vec<EncryptedData> = vec![];

    for encrypted_data in encrypted_data {
        if now
            .signed_duration_since(encrypted_data.created_at)
            .num_minutes()
            > 10
        {
            println!("{:?}", encrypted_data.transaction_state.clone());
            encrypted_transactions_to_decrypt.push(encrypted_data);
        }
    }
    Ok(encrypted_transactions_to_decrypt)
}

// This function should get unconfirmed encryped and check if they have been unconfirmed for more than 10 minutes
// If they have this should update to failed and the records related to the transaction should be updated to unspent
pub fn check_unconfirmed_transactions<N: Network>() -> AvailResult<()> {
    let expired_transactions = get_expired_unconfirmed_transactions::<N>()?;

    for expired_transaction in expired_transactions {
        let encrypted_struct = expired_transaction.to_enrypted_struct::<N>()?;
        match expired_transaction.flavour {
            EncryptedDataTypeCommon::Transaction => {
                let tx_exec: TransactionPointer<N> =
                    encrypted_struct.decrypt(VIEWSESSION.get_instance::<N>()?)?;

                if let Some(id) = expired_transaction.id {
                    handle_transaction_failed::<N>(&id.to_string(), None)?;
                }

                let spent_record_nonces = tx_exec.spent_record_pointers_nonces();
                for nonce in spent_record_nonces {
                    update_record_spent_local_via_nonce::<N>(&nonce, false)?;
                }
            }
            EncryptedDataTypeCommon::Deployment => {
                let deployment: DeploymentPointer<N> =
                    encrypted_struct.decrypt(VIEWSESSION.get_instance::<N>()?)?;

                if let Some(id) = expired_transaction.id {
                    handle_deployment_failed::<N>(&id.to_string())?;
                }

                if let Some(fee_nonce) = deployment.spent_fee_nonce {
                    update_record_spent_local_via_nonce::<N>(fee_nonce.as_str(), false)?;
                }
            }
            _ => {}
        };
    }

    Ok(())
}

pub fn handle_transaction_failed<N: Network>(
    pointer_id: &str,
    transaction_id: Option<N::TransactionID>,
) -> AvailResult<()> {
    let address = get_address::<N>()?;
    let mut transaction_pointer = get_transaction_pointer::<N>(pointer_id)?;

    let tx_id = match transaction_id {
        Some(id) => id,
        None => {
            if let Some(id) = transaction_pointer.transaction_id() {
                id
            } else {
                return Err(AvailError::new(
                    AvailErrorType::Internal,
                    "No transaction id found".to_string(),
                    "No transaction id found".to_string(),
                ));
            }
        }
    };

    transaction_pointer.update_failed_transaction(
        "Transaction remained unconfirmed and failed, no records were spent.".to_string(),
        Some(tx_id),
    );

    let encrypted_failed_transaction = transaction_pointer.to_encrypted_data(address)?;

    update_encrypted_transaction_state_by_id(
        pointer_id,
        &encrypted_failed_transaction.ciphertext,
        &encrypted_failed_transaction.nonce,
        TransactionState::Failed,
    )?;

    Ok(())
}

pub fn handle_deployment_failed<N: Network>(pointer_id: &str) -> AvailResult<()> {
    let address = get_address::<N>()?;
    let mut deployment_pointer = get_deployment_pointer::<N>(pointer_id)?;

    deployment_pointer.update_failed_deployment(
        "Deployment remained unconfirmed and failed, no records were spent.".to_string(),
    );

    let encrypted_failed_deployment = deployment_pointer.to_encrypted_data(address)?;

    update_encrypted_transaction_state_by_id(
        pointer_id,
        &encrypted_failed_deployment.ciphertext,
        &encrypted_failed_deployment.nonce,
        TransactionState::Failed,
    )?;

    Ok(())
}
/// Check if transaction pointer is already stored via transaction id
pub fn is_transcation_stored(transaction_id: &str) -> AvailResult<bool> {
    match get_encrypted_data_by_transaction_id(transaction_id) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}
#[cfg(test)]
mod tx_out_storage_api_tests {
    use super::*;
    use avail_common::models::{
        constants::*,
        encrypted_data::{EventTypeCommon, TransactionState},
    };
    use chrono::Local;
    use snarkvm::prelude::{Address, AleoID, Field, PrivateKey, TestnetV0, ToBytes, ViewKey};
    use std::str::FromStr;
    use uuid::Uuid;

    use crate::services::local_storage::{
        encrypted_data::{
            delete_user_encrypted_data, initialize_encrypted_data_table, store_encrypted_data,
        },
        session::view::VIEWSESSION,
    };

    #[test]
    fn test_store_view_session() {
        let pk = PrivateKey::<TestnetV0>::from_str(TESTNET_PRIVATE_KEY).unwrap();
        let view_key = ViewKey::<TestnetV0>::try_from(&pk).unwrap();

        VIEWSESSION.set_view_session(&view_key.to_string()).unwrap();
    }

    #[test]
    fn test_store_tx_out() {
        delete_user_encrypted_data().unwrap();
        initialize_encrypted_data_table().unwrap();

        let test_transaction_id = AleoID::<Field<TestnetV0>, TX_PREFIX>::from_str(
            "at1zux4zw83dayxtndd58skuy7qq7xg0d6ez86ak9zlqh2zru4kgggqjys70g",
        )
        .unwrap();

        let test_transaction_out = TransactionPointer::new(
            Some("Test_User".to_string()),
            Some(test_transaction_id),
            TransactionState::Confirmed,
            Some(100u32),
            Some("test_program_id".to_string()),
            Some("test_function_id".to_string()),
            vec![],
            vec![],
            Local::now(),
            Some(Local::now() + chrono::Duration::seconds(40)),
            None,
            EventTypeCommon::Send,
            None,
            None,
            None,
        );

        let address = Address::<TestnetV0>::from_str(TESTNET_ADDRESS).unwrap();
        let id = Uuid::new_v4();

        let encrypted_tx_in = test_transaction_out.to_encrypted_data(address).unwrap();

        store_encrypted_data(encrypted_tx_in).unwrap();
    }

    #[test]
    fn test_get_transactions_out() {
        test_store_tx_out();
        test_store_view_session();

        let test_transaction_id = AleoID::<Field<TestnetV0>, TX_PREFIX>::from_str(
            "at1zux4zw83dayxtndd58skuy7qq7xg0d6ez86ak9zlqh2zru4kgggqjys70g",
        )
        .unwrap();

        let test_transaction_out = TransactionPointer::new(
            Some("Test_User".to_string()),
            Some(test_transaction_id),
            TransactionState::Confirmed,
            Some(100u32),
            Some("test_program_id".to_string()),
            Some("test_function_id".to_string()),
            vec![],
            vec![],
            Local::now(),
            Some(Local::now() + chrono::Duration::seconds(40)),
            None,
            EventTypeCommon::Send,
            None,
            None,
            None,
        );

        let transactions_out = get_transactions_exec::<TestnetV0>().unwrap();

        assert_eq!(vec![test_transaction_out], transactions_out)
    }

    #[test]
    fn test_get_unconfirmed_and_failed_transaction_ids() {
        VIEWSESSION.set_view_session("AViewKey1jXL3nQ7ax6ft9qshgtTn8nXrkKNFjSBdbnjueFW5f2Gj");

        let transactions_out = get_unconfirmed_and_failed_transaction_ids::<TestnetV0>().unwrap();

        println!("{:?}", transactions_out);
    }

    #[test]
    fn test_get_transaction_ids_from_date() {
        VIEWSESSION.set_view_session("AViewKey1jXL3nQ7ax6ft9qshgtTn8nXrkKNFjSBdbnjueFW5f2Gj");

        let date = Local::now();
        let a_day_ago = date - chrono::Duration::days(1);

        let transactions_out = get_tx_ids_from_date::<TestnetV0>(a_day_ago).unwrap();

        println!("{:?}", transactions_out);
    }
}
