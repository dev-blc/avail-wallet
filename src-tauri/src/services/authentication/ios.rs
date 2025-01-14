use snarkvm::prelude::{TestnetV0, ToBytes};
use std::str::FromStr;

use avail_common::models::network::SupportedNetworks;

use crate::services::local_storage::{persistent_storage::get_network, session::view::VIEWSESSION};

#[cfg(any(target_os = "ios"))]
use crate::{models::storage::encryption::Keys, services::account::key_management::ios::search};

use avail_common::errors::{AvError, AvailErrorType, AvailResult};

#[cfg(any(target_os = "ios"))]
#[tauri::command(rename_all = "snake_case")]
pub fn ios_auth(password: Option<&str>, key_type: &str) -> AvailResult<()> {
    let network = get_network()?;

    let key = match SupportedNetworks::from_str(&network)? {
        SupportedNetworks::Testnet => search::<TestnetV0>(password, key_type)?,
        _ => search::<TestnetV0>(password, key_type)?,
    };

    let view_key_bytes = match key {
        Keys::ViewKey(key) => key.to_bytes_le()?,
        _ => {
            return Err(AvError::new(
                AvailErrorType::InvalidData,
                "Invalid Key Type".to_string(),
                "Invalid Key Type".to_string(),
            ))
        }
    };

    //TODO - Store view key session

    Ok(())
}
