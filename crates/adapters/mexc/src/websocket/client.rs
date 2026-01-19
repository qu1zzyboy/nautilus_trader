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

//! Provides a WebSocket client for connecting to the [MEXC](https://www.mexc.com) real-time API.
//!
//! MEXC uses Protocol Buffers (protobuf) for WebSocket communication, requiring binary message handling.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
        RwLock,
    },
    time::Duration,
};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use futures_util::Stream;
use nautilus_common::live::get_runtime;
use nautilus_core::{
    consts::NAUTILUS_USER_AGENT,
    env::get_or_env_var_opt,
};
use nautilus_model::{
    identifiers::AccountId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    http::USER_AGENT,
    mode::ConnectionMode,
    websocket::{
        AuthTracker, PingHandler, SubscriptionState, WebSocketClient,
        WebSocketConfig, channel_message_handler,
    },
};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    error::MexcWsError,
    handler::{FeedHandler, HandlerCommand},
    messages::NautilusWsMessage,
};
use crate::common::consts::{MEXC_WS_TOPIC_DELIMITER, MEXC_WS_URL};

/// Provides a WebSocket client for connecting to the [MEXC](https://www.mexc.com) real-time API.
///
/// Key runtime patterns:
/// - Binary protobuf message encoding/decoding
/// - Authentication handshakes are managed by the internal auth tracker
/// - The subscription state maintains pending and confirmed topics for reconnection replay
/// - User data streams require a listenkey passed as URL parameter (managed by Execution Client)
#[derive(Clone, Debug)]
pub struct MexcWebSocketClient {
    url: String,
    api_key: Option<String>,
    api_secret: Option<String>,
    heartbeat: Option<u64>,
    account_id: AccountId,
    auth_tracker: AuthTracker,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: SubscriptionState,
    tracked_subscriptions: Arc<DashMap<String, ()>>,
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
}

impl MexcWebSocketClient {
    /// Creates a new [`MexcWebSocketClient`] instance.
    ///
    /// # Arguments
    ///
    /// * `url` - Optional WebSocket URL override
    /// * `api_key` - Optional API key (for future use)
    /// * `api_secret` - Optional API secret (for future use)
    /// * `account_id` - Optional account ID (defaults to "MEXC-master")
    /// * `heartbeat` - Optional heartbeat interval in seconds
    ///
    /// # Errors
    ///
    /// Returns an error if only one of `api_key` or `api_secret` is provided (both or neither required).
    pub fn new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
    ) -> anyhow::Result<Self> {
        if api_key.is_some() != api_secret.is_some() {
            anyhow::bail!("Both `api_key` and `api_secret` must be provided together");
        }

        let account_id = account_id.unwrap_or(AccountId::from("MEXC-master"));

        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        Ok(Self {
            url: url.unwrap_or(MEXC_WS_URL.to_string()),
            api_key,
            api_secret,
            heartbeat,
            account_id,
            auth_tracker: AuthTracker::new(),
            signal: Arc::new(AtomicBool::new(false)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            task_handle: None,
            subscriptions: SubscriptionState::new(MEXC_WS_TOPIC_DELIMITER),
            tracked_subscriptions: Arc::new(DashMap::new()),
            instruments_cache: Arc::new(DashMap::new()),
        })
    }

    /// Creates a new [`MexcWebSocketClient`] with environment variable credential resolution.
    pub fn new_with_env(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
    ) -> anyhow::Result<Self> {
        let key = get_or_env_var_opt(api_key, "MEXC_API_KEY");
        let secret = get_or_env_var_opt(api_secret, "MEXC_API_SECRET");

        Self::new(url, key, secret, account_id, heartbeat)
    }

    /// Returns the websocket url being used by the client.
    #[must_use]
    pub const fn url(&self) -> &str {
        self.url.as_str()
    }

    /// Returns a value indicating whether the client is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_active()
            && !self.signal.load(Ordering::Relaxed)
    }

    /// Returns a value indicating whether the client is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_closed()
            || self.signal.load(Ordering::Relaxed)
    }

    /// Caches a single instrument.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.symbol().inner(), instrument.clone());

        if let Ok(cmd_tx) = self.cmd_tx.try_read()
            && let Err(e) = cmd_tx.send(HandlerCommand::UpdateInstrument(instrument))
        {
            log::debug!("Failed to send instrument update to handler: {e}");
        }
    }

    /// Connect to the MEXC WebSocket server.
    ///
    /// **MEXC WebSocket Connection Modes:**
    ///
    /// 1. **Public Data Stream** (default): Connect to public market data streams.
    ///    - No listenkey required
    ///    - Uses default URL: `wss://wbs-api.mexc.com/ws`
    ///    - Subscribe to topics after connection using `subscribe()`
    ///
    /// 2. **User Data Stream**: Connect to authenticated user data streams.
    ///    - Requires listenkey to be provided (created and managed by Execution Client)
    ///    - Listenkey is added to URL as query parameter
    ///    - URL format: `wss://wbs-api.mexc.com/ws?listenKey=xxx`
    ///
    /// **Note:** Unlike Binance (where listenkey is sent as subscription parameter),
    /// MEXC requires listenkey to be part of the WebSocket URL. The listenkey should
    /// be created and managed by the Execution Client layer, not the WebSocket Client.
    ///
    /// # Arguments
    ///
    /// * `listen_key` - Optional listenkey for user data streams. If provided, it will
    ///   be added to the WebSocket URL as a query parameter.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket connection fails.
    pub async fn connect(&mut self, listen_key: Option<&str>) -> Result<(), MexcWsError> {
        // If listenkey is provided, add it to URL as query parameter (MEXC-specific behavior)
        if let Some(key) = listen_key {
            let separator = if self.url.contains('?') { "&" } else { "?" };
            self.url = format!("{}{}listenKey={}", self.url, separator, key);
            log::debug!("WebSocket URL with listenkey: {}", self.url);
        } else {
            log::debug!("Connecting to public data stream (no listenkey required)");
        }

        let (client, raw_rx) = self.connect_inner().await?;

        self.connection_mode.store(client.connection_mode_atomic());

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        if let Err(e) = cmd_tx.send(HandlerCommand::SetClient(client)) {
            return Err(MexcWsError::ClientError(format!(
                "Failed to send WebSocketClient to handler: {e}"
            )));
        }

        if !self.instruments_cache.is_empty() {
            let cached_instruments: Vec<InstrumentAny> = self
                .instruments_cache
                .iter()
                .map(|entry| entry.value().clone())
                .collect();
            if let Err(e) = cmd_tx.send(HandlerCommand::InitializeInstruments(cached_instruments)) {
                log::error!("Failed to replay instruments to handler: {e}");
            }
        }

        let signal = self.signal.clone();
        let account_id = self.account_id;
        let auth_tracker = self.auth_tracker.clone();
        let subscriptions = self.subscriptions.clone();
        let cmd_tx_for_reconnect = cmd_tx.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = FeedHandler::new(
                signal.clone(),
                cmd_rx,
                raw_rx,
                out_tx,
                account_id,
                auth_tracker.clone(),
                subscriptions.clone(),
            );

            loop {
                match handler.next().await {
                    Some(NautilusWsMessage::Reconnected) => {
                        if signal.load(Ordering::Relaxed) {
                            continue;
                        }
                        log::info!("WebSocket reconnected");

                        // Resubscribe to all confirmed subscriptions
                        let topics = subscriptions.all_topics();
                        if !topics.is_empty() {
                            log::debug!(
                                "Resubscribing to confirmed subscriptions: count={}",
                                topics.len()
                            );

                            for topic in &topics {
                                subscriptions.mark_subscribe(topic.as_str());
                            }

                            // Send resubscribe command
                            if let Err(e) = cmd_tx_for_reconnect.send(HandlerCommand::Subscribe {
                                topics: topics.clone(),
                            }) {
                                log::error!("Failed to send resubscribe command: {e}");
                            }
                        }

                        continue;
                    }
                    Some(msg) => {
                        if handler.send(msg).is_err() {
                            log::error!("Failed to send message (receiver dropped)");
                            break;
                        }
                    }
                    None => {
                        if handler.is_stopped() {
                            log::debug!("Stop signal received, ending message processing");
                            break;
                        }
                        log::warn!("WebSocket stream ended unexpectedly");
                        break;
                    }
                }
            }

            log::debug!("Handler task exiting");
        });

        self.task_handle = Some(Arc::new(stream_handle));

        Ok(())
    }

    /// Connect to the WebSocket and return a message receiver.
    async fn connect_inner(
        &mut self,
    ) -> Result<
        (
            WebSocketClient,
            tokio::sync::mpsc::UnboundedReceiver<Message>,
        ),
        MexcWsError,
    > {
        let (message_handler, rx) = channel_message_handler();

        let ping_handler: PingHandler = Arc::new(move |_payload: Vec<u8>| {
            // Handler responds to pings internally via select! loop
        });

        // MEXC requires application-level heartbeat: {"method": "PING"}
        // According to MEXC docs:
        // - Client must actively send ping to keep connection alive
        // - If no valid subscription, server disconnects after 30 seconds
        // - If subscription successful but no data flow, server disconnects after 1 minute
        // - Client can send ping to keep connection alive
        // The server responds with: {"id": 0, "code": 0, "msg": "PONG"}
        let heartbeat_msg = if self.heartbeat.is_some() {
            Some(r#"{"method": "PING"}"#.to_string())
        } else {
            None
        };

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())],
            heartbeat: self.heartbeat,
            heartbeat_msg,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
        };

        let keyed_quotas = vec![];
        let client = WebSocketClient::connect(
            config,
            Some(message_handler),
            Some(ping_handler),
            None,
            keyed_quotas,
            None,
        )
        .await
        .map_err(|e| MexcWsError::ClientError(e.to_string()))?;

        Ok((client, rx))
    }

    /// Provides the internal stream as a channel-based stream.
    ///
    /// # Panics
    ///
    /// This function panics if the websocket is not connected or if `stream` has already been called.
    pub fn stream(&mut self) -> impl Stream<Item = NautilusWsMessage> + use<> {
        let rx = self
            .out_rx
            .take()
            .expect("Stream receiver already taken or not connected");
        let mut rx = Arc::try_unwrap(rx).expect("Cannot take ownership - other references exist");
        async_stream::stream! {
            while let Some(msg) = rx.recv().await {
                yield msg;
            }
        }
    }

    /// Closes the client.
    ///
    /// Note: Listenkey management (creation, keepalive, closing) should be handled
    /// by the Execution Client layer, not the WebSocket Client.
    pub async fn close(&mut self) -> Result<(), MexcWsError> {
        log::debug!("Starting close process");

        self.signal.store(true, Ordering::Relaxed);

        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
            log::debug!("Failed to send disconnect command: {e}");
        }

        if let Some(task_handle) = self.task_handle.take() {
            match Arc::try_unwrap(task_handle) {
                Ok(handle) => match tokio::time::timeout(Duration::from_secs(2), handle).await {
                    Ok(Ok(())) => log::debug!("Task handle completed successfully"),
                    Ok(Err(e)) => log::error!("Task handle encountered an error: {e:?}"),
                    Err(_) => log::warn!("Timeout waiting for task handle"),
                },
                Err(arc_handle) => {
                    arc_handle.abort();
                }
            }
        }

        log::debug!("Closed");
        Ok(())
    }

    /// Subscribe to the specified topics.
    pub async fn subscribe(&self, topics: Vec<String>) -> Result<(), MexcWsError> {
        log::debug!("Subscribing to topics: {topics:?}");

        for topic in &topics {
            self.subscriptions.mark_subscribe(topic.as_str());
            self.tracked_subscriptions.insert(topic.clone(), ());
        }

        // TODO: Build protobuf subscribe message
        // For now, just send the command
        let cmd = HandlerCommand::Subscribe { topics };

        self.cmd_tx.read().await.send(cmd).map_err(|e| {
            MexcWsError::SubscriptionError(format!("Failed to send subscribe command: {e}"))
        })
    }

    /// Unsubscribe from the specified topics.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if sending the unsubscription message fails.
    pub async fn unsubscribe(&self, topics: Vec<String>) -> Result<(), MexcWsError> {
        log::debug!("Unsubscribing from topics: {topics:?}");

        for topic in &topics {
            self.subscriptions.mark_unsubscribe(topic.as_str());
            self.tracked_subscriptions.remove(topic);
        }

        let cmd = HandlerCommand::Unsubscribe { topics };

        self.cmd_tx.read().await.send(cmd).map_err(|e| {
            MexcWsError::SubscriptionError(format!("Failed to send unsubscribe command: {e}"))
        })
    }
}
