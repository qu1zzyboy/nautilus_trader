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
    identifiers::AccountId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    RECONNECTED,
    retry::{RetryManager, create_websocket_retry_manager},
    websocket::{AuthTracker, SubscriptionState, WebSocketClient},
};
use std::str::FromStr;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use prost::Message as ProstMessage;
use serde_json;

use super::{
    enums::MexcWsChannel,
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
                                log::error!("Failed to send authentication: {e}");
                            }
                            continue;
                        }
                        Some(HandlerCommand::Subscribe { topics }) => {
                            for topic in topics {
                                if let Err(e) = self.send_subscribe_message(&topic).await {
                                    log::error!("Failed to send subscribe message for {topic}: {e}");
                                }
                            }
                            continue;
                        }
                        Some(HandlerCommand::Unsubscribe { topics }) => {
                            for topic in topics {
                                if let Err(e) = self.send_unsubscribe_message(&topic).await {
                                    log::error!("Failed to send unsubscribe message for {topic}: {e}");
                                }
                            }
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
                            log::debug!("Command channel closed");
                            return None;
                        }
                    }
                }

                msg = self.raw_rx.recv() => {
                    let msg = match msg {
                        Some(msg) => msg,
                        None => {
                            log::debug!("WebSocket stream closed");
                            return None;
                        }
                    };

                    // Handle ping frames directly for minimal latency
                    if let Message::Ping(data) = &msg {
                        log::trace!("Received ping frame with {} bytes", data.len());
                        if let Some(client) = &self.client
                            && let Err(e) = client.send_pong(data.to_vec()).await
                        {
                            log::warn!("Failed to send pong frame: {e}");
                        }
                        continue;
                    }

                    // Handle binary protobuf messages directly
                    if let Message::Binary(data) = &msg {
                        let clock = get_atomic_clock_realtime();
                        let ts_init = clock.get_time_ns();
                        
                        match Self::parse_protobuf_message(&data, &self.instruments_cache, ts_init) {
                            Some(MexcWsMessage::Data(data_vec)) => {
                                if self.signal.load(Ordering::Relaxed) {
                                    log::debug!("Stop signal received");
                                    return None;
                                }
                                return Some(NautilusWsMessage::Data(data_vec));
                            }
                            Some(MexcWsMessage::Reconnected) => {
                                return Some(NautilusWsMessage::Reconnected);
                            }
                            Some(MexcWsMessage::Subscription { success, topic, error }) => {
                                if let Some(topic) = topic {
                                    if success {
                                        log::info!("Subscription confirmed for topic: {topic}");
                                        self.subscriptions.confirm_subscribe(&topic);
                                    } else {
                                        log::error!("Subscription failed for topic: {topic}");
                                        self.subscriptions.mark_failure(&topic);
                                        if let Some(err) = error {
                                            log::error!("Subscription failed for {topic}: {err}");
                                        }
                                    }
                                }
                                continue;
                            }
                            None => {
                                continue;
                            }
                        }
                    }

                    let event = match Self::parse_raw_message(msg) {
                        Some(event) => event,
                        None => continue,
                    };

                    if self.signal.load(Ordering::Relaxed) {
                        log::debug!("Stop signal received");
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
                                        log::error!("Subscription failed for {topic}: {err}");
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
                        log::debug!("Stop signal received during idle period");
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
                    log::info!("Received WebSocket reconnected signal");
                    return Some(MexcWsMessage::Reconnected);
                }
                // MEXC may send text messages for subscription confirmations
                // Format: {"id": 0, "code": 0, "msg": "success"} for success
                // Format: {"id": 0, "code": 0, "msg": "Not Subscribed successfully! [topic]. Reason: ..."} for error
                // Also check for status field format: {"status": 200, "params": ["topic"]}
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    // Check for code field (MEXC uses code: 0 for success, non-zero for error)
                    let (success, topic, error) = if let Some(code) = json.get("code").and_then(|c| c.as_i64()) {
                        // MEXC format: {"id": 0, "code": 0, "msg": "..."}
                        let success = code == 0 && json.get("msg")
                            .and_then(|m| m.as_str())
                            .map(|m| m.contains("success") || !m.contains("Not Subscribed"))
                            .unwrap_or(false);
                        
                        // Extract topic from msg field if available
                        // Format: "Not Subscribed successfully! [spot@public.limit.depth.v3.api.pb@BTCUSDT@5]. Reason: ..."
                        let topic = json.get("msg")
                            .and_then(|m| m.as_str())
                            .and_then(|msg| {
                                // Try to extract topic from message like "[topic]"
                                if let Some(start) = msg.find('[') {
                                    if let Some(end) = msg[start+1..].find(']') {
                                        Some(msg[start+1..start+1+end].to_string())
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            })
                            .or_else(|| {
                                // Fallback: try params array
                                json.get("params")
                                    .and_then(|p| p.as_array())
                                    .and_then(|arr| arr.first())
                                    .and_then(|s| s.as_str())
                                    .map(|s| s.to_string())
                            });
                        
                        let error = if !success {
                            json.get("msg")
                                .and_then(|m| m.as_str())
                                .map(|s| s.to_string())
                        } else {
                            None
                        };
                        
                        (success, topic, error)
                    } else if let Some(status) = json.get("status").and_then(|s| s.as_u64()) {
                        // Alternative format with status field
                        let success = status == 200;
                        let topic = json.get("params")
                            .and_then(|p| p.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string());
                        let error = if !success {
                            json.get("msg")
                                .and_then(|m| m.as_str())
                                .map(|s| s.to_string())
                        } else {
                            None
                        };
                        (success, topic, error)
                    } else {
                        // Not a subscription response
                        (false, None, None)
                    };
                    
                    if topic.is_some() || error.is_some() {
                        return Some(MexcWsMessage::Subscription { success, topic, error });
                    }
                }
                None
            }
            Message::Binary(_data) => {
                // Key: Parse protobuf binary message
                // Note: We need instruments cache and timestamp, but parse_raw_message is static
                // This will be handled in the next() method instead
                None
            }
            Message::Ping(_) => {
                // Handled in select! loop before parse_raw_message
                None
            }
            Message::Pong(_) => {
                log::trace!("Received pong frame");
                None
            }
            Message::Close(_) => {
                log::debug!("Received close message, waiting for reconnection");
                None
            }
            Message::Frame(_) => {
                log::trace!("Received raw frame");
                None
            }
        }
    }

    /// Sends a subscribe message for the given topic.
    ///
    /// Topic format can be:
    /// - Full format: "spot@public.limit.depth.v3.api.pb@BTCUSDT@5" (channel@symbol@depth)
    /// - Simple format: "channel:symbol" (e.g., "spot@public.deals.v3.api:BTC_USDT")
    async fn send_subscribe_message(&self, topic: &str) -> anyhow::Result<()> {
        // MEXC subscription message format: {"method": "SUBSCRIPTION", "params": ["channel@symbol@depth"]}
        // If topic contains '@', assume it's already in the full format
        let subscribe_msg = if topic.contains('@') && !topic.contains(':') {
            // Full format: "spot@public.limit.depth.v3.api.pb@BTCUSDT@5"
            serde_json::json!({
                "method": "SUBSCRIPTION",
                "params": [topic]
            })
        } else {
            // Simple format: "channel:symbol" - convert to full format
            let (channel_str, symbol) = topic
                .split_once(':')
                .ok_or_else(|| anyhow::anyhow!("Invalid topic format: {topic}"))?;

            // Validate channel string
            let _channel = MexcWsChannel::from_str(channel_str)
                .ok_or_else(|| anyhow::anyhow!("Unknown channel: {channel_str}"))?;

            // Convert to full format: "channel@symbol"
            let full_topic = format!("{}@{}", channel_str, symbol);
            serde_json::json!({
                "method": "SUBSCRIPTION",
                "params": [full_topic]
            })
        };

        let payload = serde_json::to_string(&subscribe_msg)
            .map_err(|e| anyhow::anyhow!("Failed to serialize subscribe message: {e}"))?;

        if let Some(client) = &self.client {
            client.send_text(payload, None).await.map_err(|e| {
                anyhow::anyhow!("Failed to send subscribe message: {e}")
            })?;
        } else {
            return Err(anyhow::anyhow!("WebSocket client not available"));
        }

        Ok(())
    }

    /// Sends an unsubscribe message for the given topic.
    ///
    /// Topic format can be:
    /// - Full format: "spot@public.limit.depth.v3.api.pb@BTCUSDT@5" (channel@symbol@depth)
    /// - Simple format: "channel:symbol" (e.g., "spot@public.deals.v3.api:BTC_USDT")
    async fn send_unsubscribe_message(&self, topic: &str) -> anyhow::Result<()> {
        // MEXC unsubscription message format: {"method": "UNSUBSCRIPTION", "params": ["channel@symbol@depth"]}
        let unsubscribe_msg = if topic.contains('@') && !topic.contains(':') {
            // Full format: "spot@public.limit.depth.v3.api.pb@BTCUSDT@5"
            serde_json::json!({
                "method": "UNSUBSCRIPTION",
                "params": [topic]
            })
        } else {
            // Simple format: "channel:symbol" - convert to full format
            let (channel_str, symbol) = topic
                .split_once(':')
                .ok_or_else(|| anyhow::anyhow!("Invalid topic format: {topic}"))?;

            // Validate channel string
            let _channel = MexcWsChannel::from_str(channel_str)
                .ok_or_else(|| anyhow::anyhow!("Unknown channel: {channel_str}"))?;

            // Convert to full format: "channel@symbol"
            let full_topic = format!("{}@{}", channel_str, symbol);
            serde_json::json!({
                "method": "UNSUBSCRIPTION",
                "params": [full_topic]
            })
        };

        let payload = serde_json::to_string(&unsubscribe_msg)
            .map_err(|e| anyhow::anyhow!("Failed to serialize unsubscribe message: {e}"))?;

        if let Some(client) = &self.client {
            client.send_text(payload, None).await.map_err(|e| {
                anyhow::anyhow!("Failed to send unsubscribe message: {e}")
            })?;
        } else {
            return Err(anyhow::anyhow!("WebSocket client not available"));
        }

        Ok(())
    }

    /// Parses a protobuf binary message.
    fn parse_protobuf_message(
        data: &[u8],
        instruments: &AHashMap<Ustr, InstrumentAny>,
        ts_init: UnixNanos,
    ) -> Option<MexcWsMessage> {
        match MexcProtoMessage::decode(data) {
            Ok(proto_msg) => {
                // Convert protobuf message to Nautilus data types
                match super::parse::parse_protobuf_wrapper(&proto_msg, instruments, ts_init) {
                    Ok(data_vec) => {
                        if data_vec.is_empty() {
                            None
                        } else {
                            Some(MexcWsMessage::Data(data_vec))
                        }
                    }
                    Err(e) => {
                        log::warn!("Failed to parse protobuf wrapper: {e}");
                        None
                    }
                }
            }
            Err(e) => {
                log::error!(
                    "Failed to decode MEXC protobuf message: {e}, data_len: {}",
                    data.len()
                );
                None
            }
        }
    }
}

