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

//! Test binary for MEXC WebSocket position/account subscription.
//!
//! This binary tests subscribing to account updates (which includes position/balance information)
//! via WebSocket using listenkey authentication.
//!
//! # Usage
//!
//! ```bash
//! # Set environment variables
//! export MEXC_API_KEY="your_api_key"
//! export MEXC_API_SECRET="your_api_secret"
//!
//! # Run the test
//! cargo run --bin mexc-test-position-subscribe --package nautilus-mexc
//! ```

use std::env;
use std::sync::Arc;
use std::time::Duration;

use nautilus_mexc::{
    http::client::MexcRawHttpClient,
    websocket::client::MexcWebSocketClient,
};
use nautilus_mexc::websocket::messages::NautilusWsMessage;
use futures_util::{StreamExt, pin_mut};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    nautilus_common::logging::ensure_logging_initialized();

    log::info!("Starting MEXC WebSocket position/account subscription test");

    // Get API credentials from environment variables
    let api_key = env::var("MEXC_API_KEY")
        .map_err(|_| anyhow::anyhow!("MEXC_API_KEY environment variable not set"))?;
    let api_secret = env::var("MEXC_API_SECRET")
        .map_err(|_| anyhow::anyhow!("MEXC_API_SECRET environment variable not set"))?;

    log::info!("API Key: {}...", &api_key[..api_key.len().min(8)]);

    // Create HTTP client for listenkey management
    let http_client = MexcRawHttpClient::with_credentials(
        api_key.clone(),
        api_secret.clone(),
        "https://api.mexc.com".to_string(),
        Some(30),
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?;

    // Create listenkey for user data stream
    log::info!("Creating listen key for user data stream...");
    let listen_key_response = http_client
        .create_listen_key()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create listen key: {e}"))?;
    let listen_key = listen_key_response.listen_key;
    log::info!("Listen key created: {}...", &listen_key[..listen_key.len().min(20)]);

    // Create WebSocket client (lightweight, no listenkey management)
    let mut ws_client = MexcWebSocketClient::new(
        None,                    // url: use default
        Some(api_key.clone()),   // api_key
        Some(api_secret.clone()), // api_secret
        None,                    // account_id: use default
        None,                    // heartbeat: use default
    )?;

    log::info!("Connecting to MEXC WebSocket with listenkey...");
    // Pass listenkey to connect (will be added to URL as query parameter)
    ws_client.connect(Some(&listen_key)).await.map_err(|e| {
        anyhow::anyhow!("Failed to connect WebSocket: {e}")
    })?;
    log::info!("✓ WebSocket connected");

    // Start keepalive task (in production, this would be in Execution Client)
    let http_client_for_keepalive = Arc::new(http_client);
    let listen_key_for_keepalive = listen_key.clone();
    let keepalive_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30 * 60)); // 30 minutes
        loop {
            interval.tick().await;
            match http_client_for_keepalive.keepalive_listen_key(&listen_key_for_keepalive).await {
                Ok(()) => log::debug!("Listen key keepalive sent successfully"),
                Err(e) => log::warn!("Listen key keepalive failed: {e}"),
            }
        }
    });

    // Wait a bit for connection to stabilize
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Subscribe to account updates (includes balance/position information)
    // MEXC uses channel format: "spot@private.account.v3.api"
    // For account updates, we don't need a symbol, so we can subscribe to the channel directly
    log::info!("Subscribing to account updates (spot@private.account.v3.api)...");
    ws_client
        .subscribe(vec!["spot@private.account.v3.api".to_string()])
        .await
        .map_err(|e| anyhow::anyhow!("Failed to subscribe: {e}"))?;
    log::info!("✓ Subscription sent");

    // Start receiving messages
    log::info!("Waiting for account update messages...");
    log::info!("(Press Ctrl+C to stop)");
    
    let stream = ws_client.stream();
    pin_mut!(stream);
    let mut message_count = 0;
    let timeout = Duration::from_secs(60); // Run for 60 seconds

    let start_time = std::time::Instant::now();
    
    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    Some(NautilusWsMessage::Data(data_vec)) => {
                        message_count += 1;
                        log::info!("=== Received message #{} ===", message_count);
                        if data_vec.is_empty() {
                            log::warn!("Message received but data_vec is empty (may be unparsed account update)");
                        } else {
                            for data in data_vec {
                                log::info!("Data: {:?}", data);
                            }
                        }
                    }
                    Some(NautilusWsMessage::Reconnected) => {
                        log::info!("WebSocket reconnected");
                        // Resubscribe after reconnection
                        if let Err(e) = ws_client
                            .subscribe(vec!["spot@private.account.v3.api".to_string()])
                            .await
                        {
                            log::error!("Failed to resubscribe after reconnect: {e}");
                        }
                    }
                    None => {
                        log::warn!("Stream ended");
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

    log::info!("Received {} messages total", message_count);
    
    // Stop keepalive task
    keepalive_handle.abort();
    
    // Close listenkey (in production, this would be in Execution Client)
    log::info!("Closing listen key...");
    let http_client_for_close = MexcRawHttpClient::with_credentials(
        api_key,
        api_secret,
        "https://api.mexc.com".to_string(),
        Some(30),
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .map_err(|e| anyhow::anyhow!("Failed to create HTTP client for close: {e}"))?;
    
    if let Err(e) = http_client_for_close.close_listen_key(&listen_key).await {
        log::warn!("Failed to close listen key: {e}");
    } else {
        log::info!("✓ Listen key closed");
    }
    
    log::info!("Closing WebSocket connection...");
    ws_client.close().await.map_err(|e| {
        anyhow::anyhow!("Failed to close WebSocket: {e}")
    })?;
    log::info!("✓ WebSocket closed");

    Ok(())
}
