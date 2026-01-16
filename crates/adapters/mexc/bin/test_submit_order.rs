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

//! Test binary for MEXC Execution Client order submission.
//!
//! This binary tests submitting an order to buy 0.5 XRP on MEXC.
//!
//! # Usage
//!
//! ```bash
//! # Set environment variables
//! export MEXC_API_KEY="your_api_key"
//! export MEXC_API_SECRET="your_api_secret"
//!
//! # Run the test
//! cargo run --bin mexc-test-submit-order --package nautilus-mexc
//! ```

use std::env;
use std::time::Duration;

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::{Clock, TestClock},
    live::runner::set_exec_event_sender,
    messages::ExecutionEvent,
};
use nautilus_live::ExecutionClientCore;
use nautilus_core::UUID4;
use nautilus_model::{
    enums::{AccountType, OmsType, OrderSide, OrderType, TimeInForce},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId, Venue,
    },
    instruments::CurrencyPair,
    orders::OrderAny,
    types::{Currency, Price, Quantity},
};
use nautilus_mexc::{
    config::MexcExecClientConfig,
    execution::MexcExecutionClient,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    nautilus_common::logging::ensure_logging_initialized();

    log::info!("Starting MEXC Execution Client order submission test");

    // Get API credentials from environment variables
    let api_key = env::var("MEXC_API_KEY")
        .map_err(|_| anyhow::anyhow!("MEXC_API_KEY environment variable not set"))?;
    let api_secret = env::var("MEXC_API_SECRET")
        .map_err(|_| anyhow::anyhow!("MEXC_API_SECRET environment variable not set"))?;

    log::info!("API Key: {}...", &api_key[..api_key.len().min(8)]);

    // Set up event channel (must be set before creating client)
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(tx);

    // Create Execution Client Core
    let trader_id = TraderId::from("TRADER-001");
    let client_id = ClientId::from("MEXC-EXEC-001");
    let account_id = AccountId::from("MEXC-001");
    let venue = Venue::from("MEXC");
    let oms_type = OmsType::Netting; // MEXC spot uses netting

    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        venue,
        oms_type,
        account_id,
        AccountType::Cash, // MEXC spot is cash account
        None, // base_currency
        clock,
        cache.clone(),
    );

    // Create Execution Client Config
    let config = MexcExecClientConfig {
        trader_id,
        account_id,
        api_key: Some(api_key),
        api_secret: Some(api_secret),
        base_url_http: Some("https://api.mexc.com".to_string()),
        base_url_ws: Some("wss://wbs-api.mexc.com/ws".to_string()),
        http_proxy_url: None,
        http_timeout_secs: Some(30),
        max_retries: None,
        heartbeat_interval_secs: None,
    };

    // Create Execution Client
    let mut client = MexcExecutionClient::new(core, config)?;

    // Start and connect
    log::info!("Starting execution client...");
    client.start()?;

    log::info!("Connecting to MEXC...");
    client.connect().await?;
    log::info!("✓ Connected to MEXC");

    // Wait a bit for connection to stabilize
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Create instrument ID for XRP/USDT
    let symbol = Symbol::from("XRPUSDT");
    let instrument_id = InstrumentId::new(symbol, venue);

    // Create a basic CurrencyPair instrument (in production, fetch from API)
    let ts_init = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
    let raw_symbol = Symbol::from("XRPUSDT");
    let price_increment = Price::from("0.0001");
    let size_increment = Quantity::from("0.1");
    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        Currency::from("XRP"),
        Currency::from("USDT"),
        4, // price_precision
        1, // size_precision
        price_increment,
        size_increment,
        None, // multiplier
        None, // lot_size
        None, // max_quantity
        None, // min_quantity
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        None, // maker_fee
        None, // taker_fee
        ts_init, // ts_event
        ts_init,
    );

    // Cache the instrument
    {
        let mut cache_guard = cache.borrow_mut();
        cache_guard.add_instrument(instrument.into());
    }

    // Create order: Buy 0.5 XRP at market price
    let strategy_id = StrategyId::from("STRATEGY-001");
    // Generate unique client order ID based on timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let client_order_id = ClientOrderId::new(&format!("ORDER-{}", timestamp));
    let order_side = OrderSide::Buy;
    let order_type = OrderType::Market; // Market order
    let quantity = Quantity::from("0.5"); // 0.5 XRP
    let time_in_force = TimeInForce::Gtc;

    log::info!("Creating order: Buy 0.5 XRP (Market)");
    log::info!("  Instrument: {instrument_id}");
    log::info!("  Client Order ID: {client_order_id}");
    log::info!("  Side: {order_side:?}");
    log::info!("  Type: {order_type:?}");
    log::info!("  Quantity: {quantity}");

    // Create OrderInitialized event first
    let order_init = nautilus_model::events::OrderInitialized::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        order_side,
        order_type,
        quantity,
        time_in_force,
        false, // post_only
        false, // reduce_only
        false, // quote_quantity
        false, // reconciliation
        UUID4::new(), // event_id
        ts_init, // ts_event
        ts_init, // ts_init
        None, // price
        None, // trigger_price
        None, // trigger_type
        None, // limit_offset
        None, // trailing_offset
        None, // trailing_offset_type
        None, // expire_time
        None, // display_qty
        None, // emulation_trigger
        None, // trigger_instrument_id
        None, // contingency_type
        None, // order_list_id
        None, // linked_order_ids
        None, // parent_order_id
        None, // exec_algorithm_id
        None, // exec_algorithm_params
        None, // exec_spawn_id
        None, // tags
    );

    // Create Order from OrderInitialized
    use nautilus_model::orders::OrderAny;
    let order_any = OrderAny::from(order_init.clone());

    // Add order to cache
    {
        let mut cache_guard = cache.borrow_mut();
        cache_guard.add_order(order_any.into(), None, Some(client_id), false)?;
    }

    // Create SubmitOrder command
    let submit_cmd = nautilus_common::messages::execution::SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        client_order_id,
        order_init,
        None, // exec_algorithm_id
        None, // position_id
        None, // params
        UUID4::new(), // command_id
        ts_init,
    );

    // Submit order
    log::info!("Submitting order...");
    client.submit_order(&submit_cmd)?;
    log::info!("✓ Order submitted");

    // Listen for order events
    log::info!("Waiting for order events...");
    log::info!("(Press Ctrl+C to stop)");

    let timeout = Duration::from_secs(30);
    let start_time = std::time::Instant::now();

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Some(ExecutionEvent::Order(order_event)) => {
                        match order_event {
                            nautilus_model::events::OrderEventAny::Submitted(e) => {
                                log::info!("✓ Order Submitted: client_order_id={}", e.client_order_id);
                            }
                            nautilus_model::events::OrderEventAny::Accepted(e) => {
                                log::info!("✓ Order Accepted: client_order_id={}, venue_order_id={}", 
                                    e.client_order_id, e.venue_order_id);
                            }
                            nautilus_model::events::OrderEventAny::Rejected(e) => {
                                log::error!("✗ Order Rejected: client_order_id={}, reason={}", 
                                    e.client_order_id, e.reason);
                                break;
                            }
                            nautilus_model::events::OrderEventAny::Filled(e) => {
                                log::info!("✓ Order Filled: client_order_id={}, venue_order_id={}, last_qty={}, last_px={}", 
                                    e.client_order_id, e.venue_order_id, e.last_qty, e.last_px);
                            }
                            nautilus_model::events::OrderEventAny::Canceled(e) => {
                                log::info!("✓ Order Canceled: client_order_id={}", e.client_order_id);
                            }
                            _ => {
                                log::info!("Order event: {:?}", order_event);
                            }
                        }
                    }
                    Some(ExecutionEvent::Account(e)) => {
                        log::info!("Account state update: {:?}", e);
                    }
                    Some(e) => {
                        log::info!("Execution event: {:?}", e);
                    }
                    None => {
                        log::warn!("Event channel closed");
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(timeout) => {
                log::info!("Timeout reached ({} seconds), stopping...", timeout.as_secs());
                break;
            }
        }

        if start_time.elapsed() >= timeout {
            break;
        }
    }

    log::info!("Disconnecting...");
    client.disconnect().await?;
    log::info!("Stopping...");
    client.stop()?;
    log::info!("✓ Test completed");

    Ok(())
}

