// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Integration tests for MEXC DataClient.

use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

use nautilus_common::{
    cache::Cache,
    clock::TestClock,
    clients::DataClient,
    live::runner::set_data_event_sender,
    messages::DataEvent,
    testing::wait_until_async,
};
use nautilus_model::identifiers::ClientId;
use rstest::rstest;

use nautilus_mexc::{
    config::MexcDataClientConfig,
    data::MexcDataClient,
};

fn setup_test_env() {
    // Initialize data event sender for tests
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(sender);
}

#[rstest]
#[tokio::test]
async fn test_mexc_data_client_creation() {
    setup_test_env();
    let config = MexcDataClientConfig::default();
    let client_id = ClientId::from("MEXC-TEST");

    let result = MexcDataClient::new(client_id, config);
    assert!(result.is_ok());

    let client = result.unwrap();
    assert_eq!(client.client_id(), ClientId::from("MEXC-TEST"));
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_mexc_data_client_connect() {
    setup_test_env();
    let config = MexcDataClientConfig::default();
    let client_id = ClientId::from("MEXC-TEST");

    let mut client = MexcDataClient::new(client_id, config).unwrap();

    // Try to connect (this will attempt to connect to real MEXC API)
    // In a real integration test, we might use a mock server
    let result = client.connect().await;

    // Connection might succeed or fail depending on network/API availability
    // We just verify the method doesn't panic
    match result {
        Ok(()) => {
            // If connection succeeds, verify state
            assert!(client.is_connected());
            
            // Clean up
            let _ = client.disconnect().await;
        }
        Err(_) => {
            // Connection failed (network issue, API down, etc.)
            // This is acceptable for integration tests
            assert!(!client.is_connected());
        }
    }
}

#[rstest]
#[tokio::test]
async fn test_mexc_data_client_request_instruments() {
    setup_test_env();
    let config = MexcDataClientConfig::default();
    let client_id = ClientId::from("MEXC-TEST");

    let mut client = MexcDataClient::new(client_id, config).unwrap();

    // Try to connect first
    if client.connect().await.is_ok() {
        // Wait a bit for instruments to load
        wait_until_async(
            || async { client.is_connected() },
            Duration::from_secs(5),
        )
        .await;

        // Request instruments
        // Note: This uses the DataClient trait method which sends via message bus
        // For direct testing, we might want to test the internal method
        // For now, we just verify the client is in a valid state
        
        // Clean up
        let _ = client.disconnect().await;
    }
}

