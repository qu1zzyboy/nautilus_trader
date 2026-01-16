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

//! Example demonstrating live data testing with the MEXC adapter.
//!
//! Run with: `cargo run --example mexc-node-data-tester --package nautilus-mexc`
//!
//! This example demonstrates:
//! - Creating a LiveNode with MEXC DataClient
//! - Connecting to MEXC API
//! - Requesting instruments
//! - Subscribing to market data

use nautilus_mexc::{
    config::MexcDataClientConfig,
    factories::MexcDataClientFactory,
};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{ClientId, InstrumentId, TraderId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    nautilus_common::logging::ensure_logging_initialized();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let node_name = "MEXC-DATA-TESTER-001".to_string();

    let data_config = MexcDataClientConfig {
        api_key: None,    // Optional: for authenticated endpoints
        api_secret: None, // Optional: for authenticated endpoints
        ..Default::default()
    };

    let data_factory = MexcDataClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .with_timeout_connection(30)
        .with_delay_post_stop_secs(5)
        .build()?;

    log::info!("MEXC DataClient node built successfully");
    log::info!("Starting node...");

    // Run the node
    node.run().await?;

    Ok(())
}

