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

//! WebSocket message handler for MEXC.
//!
//! This handler processes binary protobuf messages from MEXC WebSocket streams.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::Data,
    identifiers::AccountId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    RECONNECTED,
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{AuthTracker, SubscriptionState, WebSocketClient},
};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use prost::Message as ProstMessage;

use super::{
    error::MexcWsError,
    messages::{MexcWsMessage, NautilusWsMessage},
};
use crate::proto::MexcProtoMessage;

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
pub enum HandlerCommand {
    /// Set the WebSocketClient for the handler to use.
    SetClient(WebSocketClient),
    /// Disconnect the WebSocket connection.
    Disconnect,
    /// Send authentication payload to the WebSocket.
    Authenticate { payload: Vec<u8> },
    /// Subscribe to the given topics.
    Subscribe { topics: Vec<String> },
    /// Unsubscribe from the given topics.
    Unsubscribe { topics: Vec<String> },
    /// Initialize the instruments cache with the given instruments.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the cache.
    UpdateInstrument(InstrumentAny),
}

pub(super) struct FeedHandler {
    account_id: AccountId,
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    auth_tracker: AuthTracker,
    subscriptions: SubscriptionState,
    retry_manager: RetryManager<MexcWsError>,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        account_id: AccountId,
        auth_tracker: AuthTracker,
        subscriptions: SubscriptionState,
    ) -> Self {
        Self {
            account_id,
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            out_tx,
            auth_tracker,
            subscriptions,
            retry_manager: create_websocket_retry_manager(),
            instruments_cache: AHashMap::new(),
        }
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    pub(super) fn send(&self, msg: NautilusWsMessage) -> Result<(), ()> {
        self.out_tx.send(msg).map_err(|_| ())
    }

    /// Sends a WebSocket binary message with retry logic.
    async fn send_with_retry(&self, payload: Vec<u8>) -> anyhow::Result<()> {
        if let Some(client) = &self.client {
            self.retry_manager
                .execute_with_retry(
                    "websocket_send",
                    || {
                        let payload = payload.clone();
                        async move {
                            client.send_bytes(payload, None).await.map_err(|e| {
                                MexcWsError::ClientError(format!("Send failed: {e}"))
                            })
                        }
                    },
                    |_| true, // Always retry on error
                    |msg: String| MexcWsError::ClientError(msg),
                )
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        Ok(())
    }

    /// Processes raw WebSocket messages and returns Nautilus messages.
    pub(super) async fn next(&mut self) -> Option<NautilusWsMessage> {
        let clock = get_atomic_clock_realtime();

        loop {
            tokio::select! {
                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(HandlerCommand::SetClient(client)) => {
                            self.client = Some(client);
                            continue;
                        }
                        Some(HandlerCommand::Disconnect) => {
                            if let Some(client) = &self.client {
                                let _ = client.disconnect().await;
                            }
                            return None;
                        }
                        Some(HandlerCommand::Authenticate { payload }) => {
                            if let Err(e) = self.send_with_retry(payload).await {
                                tracing::error!("Failed to send authentication: {e}");
                            }
                            continue;
                        }
                        Some(HandlerCommand::Subscribe { topics }) => {
                            // TODO: Build protobuf subscribe message and send
                            tracing::debug!("Subscribe command received for topics: {topics:?}");
                            continue;
                        }
                        Some(HandlerCommand::Unsubscribe { topics }) => {
                            // TODO: Build protobuf unsubscribe message and send
                            tracing::debug!("Unsubscribe command received for topics: {topics:?}");
                            continue;
                        }
                        Some(HandlerCommand::InitializeInstruments(instruments)) => {
                            for inst in instruments {
                                self.instruments_cache.insert(inst.symbol().inner(), inst);
                            }
                            continue;
                        }
                        Some(HandlerCommand::UpdateInstrument(inst)) => {
                            self.instruments_cache.insert(inst.symbol().inner(), inst);
                            continue;
                        }
                        None => {
                            tracing::debug!("Command channel closed");
                            return None;
                        }
                    }
                }

                msg = self.raw_rx.recv() => {
                    let msg = match msg {
                        Some(msg) => msg,
                        None => {
                            tracing::debug!("WebSocket stream closed");
                            return None;
                        }
                    };

                    // Handle ping frames directly for minimal latency
                    if let Message::Ping(data) = &msg {
                        tracing::trace!("Received ping frame with {} bytes", data.len());
                        if let Some(client) = &self.client
                            && let Err(e) = client.send_pong(data.to_vec()).await
                        {
                            tracing::warn!(error = %e, "Failed to send pong frame");
                        }
                        continue;
                    }

                    let event = match Self::parse_raw_message(msg) {
                        Some(event) => event,
                        None => continue,
                    };

                    if self.signal.load(Ordering::Relaxed) {
                        tracing::debug!("Stop signal received");
                        return None;
                    }

                    match event {
                        MexcWsMessage::Reconnected => {
                            return Some(NautilusWsMessage::Reconnected);
                        }
                        MexcWsMessage::Subscription { success, topic, error } => {
                            if let Some(topic) = topic {
                                if success {
                                    self.subscriptions.confirm_subscribe(&topic);
                                } else {
                                    self.subscriptions.mark_failure(&topic);
                                    if let Some(err) = error {
                                        tracing::error!("Subscription failed for {topic}: {err}");
                                    }
                                }
                            }
                            continue;
                        }
                        MexcWsMessage::Data(data) => {
                            return Some(NautilusWsMessage::Data(data));
                        }
                    }
                }

                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if self.signal.load(Ordering::Relaxed) {
                        tracing::debug!("Stop signal received during idle period");
                        return None;
                    }
                    continue;
                }
            }
        }
    }

    /// Parses a raw WebSocket message into an internal message type.
    ///
    /// For MEXC, this handles binary protobuf messages.
    fn parse_raw_message(msg: Message) -> Option<MexcWsMessage> {
        match msg {
            Message::Text(text) => {
                if text == RECONNECTED {
                    tracing::info!("Received WebSocket reconnected signal");
                    return Some(MexcWsMessage::Reconnected);
                }
                // MEXC may send some text messages (e.g., subscription confirmations)
                // TODO: Parse text messages if needed
                tracing::trace!("Received text message: {text}");
                None
            }
            Message::Binary(data) => {
                // Key: Parse protobuf binary message
                Self::parse_protobuf_message(&data)
            }
            Message::Ping(_) => {
                // Handled in select! loop before parse_raw_message
                None
            }
            Message::Pong(_) => {
                tracing::trace!("Received pong frame");
                None
            }
            Message::Close(_) => {
                tracing::debug!("Received close message, waiting for reconnection");
                None
            }
            Message::Frame(_) => {
                tracing::trace!("Received raw frame");
                None
            }
        }
    }

    /// Parses a protobuf binary message.
    fn parse_protobuf_message(data: &[u8]) -> Option<MexcWsMessage> {
        match MexcProtoMessage::decode(data) {
            Ok(proto_msg) => {
                // TODO: Convert protobuf message to internal message type
                tracing::trace!("Successfully decoded protobuf message, size: {} bytes", data.len());
                // Placeholder - implement actual conversion
                None
            }
            Err(e) => {
                tracing::error!(
                    "Failed to decode MEXC protobuf message: {e}, data_len: {}",
                    data.len()
                );
                None
            }
        }
    }
}

