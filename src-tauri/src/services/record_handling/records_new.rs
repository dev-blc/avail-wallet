use avail_common::{aleo_tools::api, errors::{AvailError, AvailResult}};
use snarkvm::prelude::{
	Network, Testnet3, Transaction, ToField, FromStr, ViewKey, Parser, Field, Group, Scalar
};
use crate::api::{aleo_client::setup_client, client};
use serde_json::Value;
use tauri_plugin_http::reqwest::Client;

struct LocalClient {
	client: Client,
	api_key: String,
	base_url: String,
	network_id: String
}

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
		println!("response: {:?}\n", response);

		// Get content from response
		let content = response.text().await.unwrap();

		// Parse the content as JSON
		serde_json::from_str::<Value>(&content).unwrap().as_array().unwrap().clone()
	}

	// Return a transaction
	async fn get_transaction_from_transition(&self, transition_id: &str) -> String {
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
}

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

pub async fn get_records_new<N: Network>(
    start: u32,
    end: u32,
    view_key_str: &str
) -> AvailResult<()> {
    // Prepare API client and get records
    let mut api_key: String = "bcde0fb4-a4fa-4e84-affd-ab70b5e477db".to_string();
	let mut client = LocalClient::new(
		api_key, 
		"https://aleo-testnet3.dev.obscura.network".to_string(), 
		"testnet3".to_string()
	);
    let records = client.get_records(start, end).await;

	for record in records {
		println!("record: {:?}\n", record);

		// Get values from record and cast to primitive types
		let (_, nonce_x) = Field::<N>::parse(record.get("nonce_x").unwrap().as_str().unwrap()).unwrap();
		let (_, nonce_y) = Field::<N>::parse(record.get("nonce_y").unwrap().as_str().unwrap()).unwrap();
		let (_, owner_x) = Field::<N>::parse(record.get("owner_x").unwrap().as_str().unwrap()).unwrap();
		let nonce = Group::<N>::from_xy_coordinates(nonce_x, nonce_y);
		let view_key = ViewKey::<N>::from_str(view_key_str).unwrap();
		let address = view_key.to_address().to_field().unwrap();

		// Check if the record is owned
		if (is_owner_direct(address, *view_key, nonce, owner_x)) {
			println!("Found record owned!{}\n", record);

			// Get transition ID
			let transition_id = record.get("transition_id").unwrap().as_str().unwrap();

			// Different API key for getting transaction 
			api_key = env!("TESTNET_API_OBSCURA").to_string();
			client.change_api_key(api_key);
			client.change_base_url("https://aleo-testnet3.obscura.network".to_string());

			// Get transaction ID and transaction
			let transaction_id = client.get_transaction_from_transition(transition_id).await;
			println!("Transaction ID: {:?}\n", transaction_id);
			let transaction = client.get_transaction(&transaction_id).await;
			print!("Transaction: {:?}\n", transaction);
		};
	}

    Ok(())
}

#[cfg(test)]
mod record_handling_tests {
	use super::*;

	#[tokio::test]
	async fn test_get_records_new() {
		let view_key = env!("VIEW_KEY");
		let current_block: u32 = 2087986;
		let start: u32 = current_block - 10;
		let success: Result<(), AvailError> = get_records_new::<Testnet3>(start, current_block, view_key).await;

		assert!(success.is_ok());
	}
}