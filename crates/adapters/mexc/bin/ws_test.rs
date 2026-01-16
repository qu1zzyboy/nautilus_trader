// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{env, time::Duration};

use futures_util::StreamExt;
use nautilus_mexc::websocket::client::MexcWebSocketClient;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::CurrencyPair,
    types::{Currency, Price, Quantity},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let args: Vec<String> = env::args().collect();
    let symbol = args.get(1).map_or("BTCUSDT", String::as_str);
    let depth = args.get(2).map_or("5", String::as_str);

    log::info!("Starting MEXC WebSocket test");
    log::info!("Symbol: {symbol}");
    log::info!("Depth: {depth}");

    // Create a basic instrument for testing
    // Note: In production, you would fetch this from the HTTP API
    // MEXC spot trading uses CurrencyPair
    let venue = nautilus_mexc::MEXC_VENUE.clone();
    let instrument_id = InstrumentId::new(
        Symbol::from(symbol),
        venue,
    );
    
    // Parse symbol to get base and quote currencies
    // For BTCUSDT: base = BTC, quote = USDT
    let (base_str, quote_str) = if symbol.ends_with("USDT") {
        let base = &symbol[..symbol.len() - 4];
        (base, "USDT")
    } else if symbol.ends_with("USD") {
        let base = &symbol[..symbol.len() - 3];
        (base, "USD")
    } else {
        // Default: assume last 4 chars are quote
        let base = &symbol[..symbol.len().saturating_sub(4)];
        let quote = &symbol[symbol.len().saturating_sub(4)..];
        (base, quote)
    };
    
    let base_currency = Currency::from(base_str);
    let quote_currency = Currency::from(quote_str);
    
    // Create a basic currency pair instrument
    // Using default values for testing - in production these should come from API
    let ts_init = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
    let instrument = CurrencyPair::new(
        instrument_id,
        Symbol::from(symbol),
        base_currency,
        quote_currency,
        8, // price_precision (default)
        8, // size_precision (default)
        Price::new(0.01, 8), // price_increment
        Quantity::new(0.00001, 8), // size_increment
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
        ts_init,
        ts_init,
    )
    .into();

    // Create WebSocket client
    let mut ws_client = MexcWebSocketClient::new(
        None,  // url: defaults to wss://wbs.mexc.com/ws
        None,  // api_key: not needed for public data
        None,  // api_secret: not needed for public data
        None,  // account_id
        Some(5), // 5 second heartbeat
    )?;

    // Cache the instrument
    ws_client.cache_instrument(instrument);

    log::info!("Connecting to MEXC WebSocket...");
    ws_client.connect(None).await?; // None = no listenkey (public data stream)

    // Give the connection a moment to stabilize
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Subscribe to 5-level order book snapshot
    // Format: "spot@public.limit.depth.v3.api.pb@BTCUSDT@5"
    let topic = format!("spot@public.limit.depth.v3.api.pb@{symbol}@{depth}");
    log::info!("Subscribing to topic: {topic}");

    if let Err(e) = ws_client.subscribe(vec![topic.clone()]).await {
        log::error!("Failed to subscribe: {e}");
        return Err(e.into());
    }

    log::info!("Subscription sent, waiting for data...");
    log::info!("Press CTRL+C to stop");

    // Create a future that completes on CTRL+C
    let sigint = tokio::signal::ctrl_c();
    tokio::pin!(sigint);

    let stream = ws_client.stream();
    tokio::pin!(stream);
    let mut message_count = 0u64;

    loop {
        tokio::select! {
            Some(msg) = stream.next() => {
                message_count += 1;
                log::info!("[Message #{message_count}] Received: {msg:?}");
                
                // Print more details for data messages
                if let nautilus_mexc::websocket::messages::NautilusWsMessage::Data(data_vec) = &msg {
                    for data in data_vec {
                        log::info!("  Data: {data:?}");
                    }
                }
            }
            _ = &mut sigint => {
                log::info!("Received SIGINT, closing connection...");
                break;
            }
            else => {
                log::warn!("Stream ended unexpectedly");
                break;
            }
        }
    }

    log::info!("Total messages received: {message_count}");
    ws_client.close().await?;
    log::info!("Connection closed successfully");

    Ok(())
}

