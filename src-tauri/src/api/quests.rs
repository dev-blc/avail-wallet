use std::str::FromStr;

use avail_common::models::encrypted_data::EncryptedDataTypeCommon;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::api::client::get_quest_client_with_session;
use crate::models::pointers::{
    deployment::DeploymentPointer, transaction::TransactionPointer, transition::TransitionPointer,
};
use crate::services::local_storage::persistent_storage::get_network;
use crate::services::local_storage::session::view::VIEWSESSION;
use crate::services::local_storage::storage_api::transaction::get_transaction_ids_for_quest_verification;
use avail_common::{
    errors::{AvailError, AvailErrorType, AvailResult},
    models::encrypted_data::EventTypeCommon,
    models::network::SupportedNetworks,
    models::quests::*,
};
use tauri_plugin_http::reqwest;

use snarkvm::prelude::{Network, Testnet3, Transaction};

use super::aleo_client::setup_client;

/* GET ALL CAMPAIGNS */
#[tauri::command(rename_all = "snake_case")]
pub async fn get_campaigns() -> AvailResult<Vec<Campaign>> {
    let res = get_quest_client_with_session(reqwest::Method::GET, "campaigns")?
        .send()
        .await?;

    if res.status() == 200 {
        let campaigns: Vec<Campaign> = res.json().await?;

        Ok(campaigns)
    } else if res.status() == 401 {
        Err(AvailError::new(
            AvailErrorType::Unauthorized,
            "User session has expired.".to_string(),
            "Your session has expired, please authenticate again.".to_string(),
        ))
    } else {
        Err(AvailError::new(
            AvailErrorType::External,
            "Error getting campaigns".to_string(),
            "Error getting campaigns".to_string(),
        ))
    }
}

/* GET ALL QUESTS FOR CAMPAIGN */
#[tauri::command(rename_all = "snake_case")]
pub async fn get_quests_for_campaign(campaign_id: &str) -> AvailResult<Vec<Quest>> {
    let res =
        get_quest_client_with_session(reqwest::Method::GET, &format!("campaign/{}", campaign_id))?
            .send()
            .await?;

    if res.status() == 200 {
        let quests: Vec<Quest> = res.json().await?;

        Ok(quests)
    } else if res.status() == 401 {
        Err(AvailError::new(
            AvailErrorType::Unauthorized,
            "User session has expired.".to_string(),
            "Your session has expired, please authenticate again.".to_string(),
        ))
    } else {
        Err(AvailError::new(
            AvailErrorType::External,
            "Error getting quests".to_string(),
            "Error getting quests".to_string(),
        ))
    }
}

/* CHECK IF QUEST IS COMPLETE */
#[tauri::command(rename_all = "snake_case")]
pub async fn check_quest_completion(quest_id: &str) -> AvailResult<bool> {
    let res =
        get_quest_client_with_session(reqwest::Method::GET, &format!("confirmed/{}", quest_id))?
            .send()
            .await?;

    if res.status() == 200 {
        let completion: VerifyTaskResponse = res.json().await?;

        Ok(completion.verified)
    } else if res.status() == 401 {
        Err(AvailError::new(
            AvailErrorType::Unauthorized,
            "User session has expired.".to_string(),
            "Your session has expired, please authenticate again.".to_string(),
        ))
    } else {
        Err(AvailError::new(
            AvailErrorType::External,
            "Error checking quest completion".to_string(),
            "Error checking quest completion".to_string(),
        ))
    }
}

/* CHECK IF TASK HAS ALREADY BEEN VERIFIED COMPLETED AND VERIFIED*/
#[tauri::command(rename_all = "snake_case")]
pub async fn is_task_verified(task_id: Uuid) -> AvailResult<bool> {
    let res =
        get_quest_client_with_session(reqwest::Method::GET, &format!("verified/{}", task_id))?
            .send()
            .await?;

    if res.status() == 200 {
        let completion: VerifyTaskResponse = res.json().await?;

        Ok(completion.verified)
    } else if res.status() == 401 {
        Err(AvailError::new(
            AvailErrorType::Unauthorized,
            "User session has expired.".to_string(),
            "Your session has expired, please authenticate again.".to_string(),
        ))
    } else {
        Err(AvailError::new(
            AvailErrorType::External,
            "Error checking quest completion".to_string(),
            "Error checking quest completion".to_string(),
        ))
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn verify_task(
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    task_id: &str,
    program_id: &str,
    function_id: &str,
) -> AvailResult<bool> {
    let network = get_network()?;

    match SupportedNetworks::from_str(network.as_str())? {
        SupportedNetworks::Testnet3 => {
            verify_task_raw::<Testnet3>(start_time, end_time, task_id, program_id, function_id)
                .await
        }
    }
}

/* CHECK IF TASK IS COMPLETE */
async fn verify_task_raw<N: Network>(
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    task_id: &str,
    program_id: &str,
    function_id: &str,
) -> AvailResult<bool> {
    let view_key = VIEWSESSION.get_instance::<N>()?;
    let task_id = Uuid::parse_str(task_id)?;
    let is_task_verified = is_task_verified(task_id).await?;

    println!("is_task_verified: {}", is_task_verified);
    if is_task_verified {
        return Ok(true);
    }

    let encrypted_transactions = get_transaction_ids_for_quest_verification::<N>(
        start_time,
        end_time,
        program_id,
        function_id,
    )?;

    println!("encrypted_transactions: {:?}", encrypted_transactions);

    if encrypted_transactions.is_empty() {
        return Ok(false);
    }

    println!("PAST THE DEMON!");

    let mut transaction_ids: Vec<N::TransactionID> = vec![];
    let mut block_heights: Vec<u32> = vec![];
    let aleo_client = setup_client::<N>()?;

    for encrypted_transaction in encrypted_transactions {
        println!("Into the Dungeon!");
        let encrypted_struct = encrypted_transaction.to_enrypted_struct::<N>()?;

        match encrypted_transaction.flavour {
            EncryptedDataTypeCommon::Transition => {
                let transition: TransitionPointer<N> = encrypted_struct.decrypt(view_key)?;
                transaction_ids.push(transition.transaction_id);
                block_heights.push(transition.block_height);
            }
            EncryptedDataTypeCommon::Transaction => {
                let tx_exec: TransactionPointer<N> =
                    encrypted_struct.decrypt(VIEWSESSION.get_instance::<N>()?)?;

                if let Some(tx_id) = tx_exec.transaction_id() {
                    transaction_ids.push(tx_id);
                }
                block_heights.push(tx_exec.block_height().unwrap_or(0));
            }
            EncryptedDataTypeCommon::Deployment => {
                let deployment: DeploymentPointer<N> =
                    encrypted_struct.decrypt(VIEWSESSION.get_instance::<N>()?)?;
                if let Some(tx_id) = deployment.id {
                    transaction_ids.push(tx_id);
                }
                block_heights.push(deployment.block_height.unwrap_or(0));
            }
            _ => {}
        };
    }

    let transaction = aleo_client.get_transaction(transaction_ids[0])?;

    for transition in transaction.transitions() {
        if transition.program_id().to_string().as_str() == program_id
            && transition.function_name().to_string().as_str() == function_id
        {
            let tpk = transition.tpk();
            let scalar = *view_key;
            let tvk = (*tpk * scalar).to_x_coordinate();

            let request = VerifyTaskRequest::<N> {
                task_id,
                confirmation_height: block_heights[0],
                transaction_id: transaction.id(),
                transition_id: *transition.id(),
                tvk,
            };

            println!("TASK VERIF Request: {:?}", request);
            let res = get_quest_client_with_session(reqwest::Method::POST, "verify")?
                .json(&request)
                .send()
                .await?;

            if res.status() == 200 {
                let completion: VerifyTaskResponse = res.json().await?;

                return Ok(completion.verified);
            } else if res.status() == 401 {
                return Err(AvailError::new(
                    AvailErrorType::Unauthorized,
                    "User session has expired.".to_string(),
                    "Your session has expired, please authenticate again.".to_string(),
                ));
            } else {
                return Err(AvailError::new(
                    AvailErrorType::External,
                    "Error checking quest completion".to_string(),
                    "Error checking quest completion".to_string(),
                ));
            }
        }
    }

    Ok(false)
}

/* GET USER'S POINTS */
#[tauri::command(rename_all = "snake_case")]
pub async fn get_points() -> AvailResult<i32> {
    let res = get_quest_client_with_session(reqwest::Method::GET, "points")?
        .send()
        .await?;

    if res.status() == 200 {
        let points: PointsResponse = res.json().await?;

        Ok(points.points)
    } else if res.status() == 401 {
        Err(AvailError::new(
            AvailErrorType::Unauthorized,
            "User session has expired.".to_string(),
            "Your session has expired, please authenticate again.".to_string(),
        ))
    } else {
        Err(AvailError::new(
            AvailErrorType::External,
            "Error getting points".to_string(),
            "Error getting points".to_string(),
        ))
    }
}

/* GET USER'S WHITELIST */
#[tauri::command(rename_all = "snake_case")]
pub async fn get_whitelists() -> AvailResult<WhitelistResponse> {
    let res = get_quest_client_with_session(reqwest::Method::GET, "whitelists")?
        .send()
        .await?;

    if res.status() == 200 {
        let whitelists: WhitelistResponse = res.json().await?;

        Ok(whitelists)
    } else if res.status() == 401 {
        Err(AvailError::new(
            AvailErrorType::Unauthorized,
            "User session has expired.".to_string(),
            "Your session has expired, please authenticate again.".to_string(),
        ))
    } else {
        Err(AvailError::new(
            AvailErrorType::External,
            "Error getting whitelists".to_string(),
            "Error getting whitelists".to_string(),
        ))
    }
}
