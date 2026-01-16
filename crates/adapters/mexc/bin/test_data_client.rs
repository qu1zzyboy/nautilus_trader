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

//! Test program for MEXC DataClient to verify instrument parsing and connection.

use std::time::Duration;

use nautilus_model::identifiers::ClientId;
use nautilus_common::clients::DataClient;
use nautilus_common::live::runner::set_data_event_sender;
use nautilus_common::messages::DataEvent;
use nautilus_mexc::{
    config::MexcDataClientConfig,
    data::MexcDataClient,
    http::client::MexcRawHttpClient,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    nautilus_common::logging::ensure_logging_initialized();
    
    // Initialize data event sender for testing
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(sender);

    log::info!("Starting MEXC DataClient test");
    log::info!("This test will:");
    log::info!("  1. Create a MEXC DataClient");
    log::info!("  2. Connect to MEXC API");
    log::info!("  3. Request and parse instruments");
    log::info!("  4. Display instrument count and sample instruments");
    log::info!("");

    // Create configuration
    let config = MexcDataClientConfig::default();
    let client_id = ClientId::from("MEXC-DATA-TEST");

    log::info!("Creating MEXC DataClient with client_id={}", client_id);
    let mut client = MexcDataClient::new(client_id, config.clone())?;
    log::info!("DataClient created successfully");

    // Test connection
    log::info!("Connecting to MEXC...");
    match client.connect().await {
        Ok(()) => {
            log::info!("✓ Successfully connected to MEXC");
        }
        Err(e) => {
            log::error!("✗ Failed to connect to MEXC: {e:?}");
            anyhow::bail!("Connection failed: {e}");
        }
    }

    // Wait a bit for instruments to be loaded
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Check connection status
    if client.is_connected() {
        log::info!("✓ Client is connected");
    } else {
        log::warn!("⚠ Client connection status is false");
    }

    // Test instrument parsing directly via HTTP client
    log::info!("Testing instrument parsing via HTTP client...");
    let http_client = MexcRawHttpClient::new(
        Some(config.http_base_url().to_string()),
        config.http_timeout_secs,
        config.max_retries,
        None, // retry_delay_ms
        None, // retry_delay_max_ms
        None, // max_requests_per_second
        None, // max_requests_per_minute
        config.http_proxy_url.clone(),
    )?;
    
    match http_client.get_exchange_info(None).await {
        Ok(mexc_instruments) => {
            log::info!("✓ Successfully fetched {} instruments from MEXC API", mexc_instruments.len());
            
            // Parse instruments
            let ts_init = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
            let mut parsed_count = 0;
            let mut inactive_count = 0;
            let mut failed_count = 0;
            let mut sample_instruments = Vec::new();
            
            // Check first few instruments to see their status
            for (i, mexc_instrument) in mexc_instruments.iter().take(5).enumerate() {
                log::info!(
                    "Sample instrument {}: symbol={}, status={:?}, base={:?}, quote={:?}",
                    i + 1,
                    mexc_instrument.symbol,
                    mexc_instrument.status,
                    mexc_instrument.base_currency,
                    mexc_instrument.quote_currency
                );
            }
            
            for mexc_instrument in &mexc_instruments {
                match nautilus_mexc::http::parse::parse_instrument_any(mexc_instrument, ts_init) {
                    nautilus_mexc::http::parse::InstrumentParseResult::Ok(boxed) => {
                        parsed_count += 1;
                        if sample_instruments.len() < 10 {
                            sample_instruments.push(*boxed);
                        }
                    }
                    nautilus_mexc::http::parse::InstrumentParseResult::Inactive { symbol, reason } => {
                        inactive_count += 1;
                        if inactive_count <= 3 {
                            log::debug!("Inactive instrument: {} - {}", symbol, reason);
                        }
                    }
                    nautilus_mexc::http::parse::InstrumentParseResult::Unsupported { .. } => {
                        // Skip unsupported
                    }
                    nautilus_mexc::http::parse::InstrumentParseResult::Failed { symbol, error, .. } => {
                        failed_count += 1;
                        if failed_count <= 3 {
                            log::debug!("Failed instrument: {} - {}", symbol, error);
                        }
                    }
                }
            }
            
            log::info!("Instrument parsing results:");
            log::info!("  ✓ Successfully parsed: {}", parsed_count);
            log::info!("  ⚠ Inactive: {}", inactive_count);
            log::info!("  ✗ Failed: {}", failed_count);
            
            if !sample_instruments.is_empty() {
                log::info!("Sample parsed instruments (first {}):", sample_instruments.len());
                for (i, instrument) in sample_instruments.iter().enumerate() {
                    use nautilus_model::instruments::Instrument;
                    log::info!(
                        "  {}. {} - {} ({})",
                        i + 1,
                        instrument.id(),
                        instrument.raw_symbol(),
                        instrument.venue()
                    );
                    if let nautilus_model::instruments::InstrumentAny::CurrencyPair(cp) = instrument {
                        use nautilus_model::instruments::Instrument;
                        log::info!(
                            "     Base: {:?}, Quote: {}, Tick: {}, Step: {}",
                            cp.base_currency(),
                            cp.quote_currency(),
                            cp.price_increment(),
                            cp.size_increment()
                        );
                    }
                }
            }
        }
        Err(e) => {
            log::error!("✗ Failed to fetch instruments from MEXC API: {e:?}");
            anyhow::bail!("Failed to fetch instruments: {e}");
        }
    }

    // Keep connection alive for a few seconds to see if WebSocket works
    log::info!("Keeping connection alive for 5 seconds to test WebSocket...");
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Disconnect
    log::info!("Disconnecting...");
    match client.disconnect().await {
        Ok(()) => {
            log::info!("✓ Successfully disconnected");
        }
        Err(e) => {
            log::error!("✗ Error during disconnect: {e:?}");
        }
    }

    log::info!("Test completed successfully!");
    Ok(())
}

