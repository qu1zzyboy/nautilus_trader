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

//! MEXC data client implementation.

use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use anyhow::Context;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent,
        data::{
            DataResponse, InstrumentResponse, InstrumentsResponse, RequestBars, RequestInstrument,
            RequestInstruments, RequestTrades, SubscribeBars, SubscribeBookDeltas,
            SubscribeInstrument, SubscribeInstruments, SubscribeQuotes, SubscribeTrades,
            UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    MUTEX_POISONED,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::Data,
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    common::consts::MEXC_VENUE,
    config::MexcDataClientConfig,
    http::client::MexcRawHttpClient,
    websocket::{
        client::MexcWebSocketClient,
        enums::MexcWsChannel,
        messages::NautilusWsMessage,
    },
};

use super::{
    bar_interval_to_mexc_interval, format_mexc_stream, format_mexc_symbol, upsert_instrument,
};

/// MEXC data client for market data streams.
#[derive(Debug)]
pub struct MexcDataClient {
    clock: &'static AtomicTime,
    client_id: ClientId,
    config: MexcDataClientConfig,
    http_client: MexcRawHttpClient,
    ws_client: MexcWebSocketClient,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
}

impl MexcDataClient {
    /// Creates a new [`MexcDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(client_id: ClientId, config: MexcDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let http_client = MexcRawHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.max_retries,
            None, // retry_delay_ms
            None, // retry_delay_max_ms
            None, // max_requests_per_second
            None, // max_requests_per_minute
            config.http_proxy_url.clone(),
        )?;

        let ws_client = MexcWebSocketClient::new(
            config.base_url_ws.clone(),
            config.api_key.clone(),
            config.api_secret.clone(),
            None, // account_id
            config.heartbeat_interval_secs,
        )?;

        Ok(Self {
            clock,
            client_id,
            config,
            http_client,
            ws_client,
            data_sender,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            instruments: Arc::new(RwLock::new(AHashMap::new())),
        })
    }

    fn venue(&self) -> Venue {
        *MEXC_VENUE
    }

    fn send_data(sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>, data: Data) {
        if let Err(e) = sender.send(DataEvent::Data(data)) {
            log::error!("Failed to emit data event: {e}");
        }
    }

    fn spawn_ws<F>(&self, fut: F, context: &'static str)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("{context}: {e:?}");
            }
        });
    }

    fn handle_ws_message(
        message: NautilusWsMessage,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        _instruments: &Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    ) {
        match message {
            NautilusWsMessage::Data(data_vec) => {
                for data in data_vec {
                    Self::send_data(data_sender, data);
                }
            }
            NautilusWsMessage::Reconnected => {
                log::info!("WebSocket reconnected");
            }
        }
    }

    /// Requests instruments from MEXC exchange info endpoint.
    async fn request_instruments_internal(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        let mexc_instruments = self
            .http_client
            .get_exchange_info(None)
            .await
            .context("failed to request MEXC exchange info")?;

        let ts_init = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
        let mut instruments = Vec::with_capacity(mexc_instruments.len());

        for mexc_instrument in &mexc_instruments {
            match crate::http::parse::parse_instrument_any(mexc_instrument, ts_init) {
                crate::http::parse::InstrumentParseResult::Ok(boxed) => {
                    instruments.push(*boxed);
                }
                crate::http::parse::InstrumentParseResult::Inactive { symbol, reason } => {
                    log::debug!(
                        "Skipping inactive instrument: symbol={}, reason={}",
                        symbol,
                        reason
                    );
                }
                crate::http::parse::InstrumentParseResult::Unsupported {
                    symbol,
                    instrument_type,
                } => {
                    log::debug!(
                        "Skipping unsupported instrument: symbol={}, type={}",
                        symbol,
                        instrument_type
                    );
                }
                crate::http::parse::InstrumentParseResult::Failed {
                    symbol,
                    instrument_type,
                    error,
                } => {
                    log::warn!(
                        "Failed to parse instrument: symbol={}, type={}, error={}",
                        symbol,
                        instrument_type,
                        error
                    );
                }
            }
        }

        log::info!("Loaded MEXC spot instruments: count={}", instruments.len());
        Ok(instruments)
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for MexcDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.venue())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Started: client_id={}, environment={:?}",
            self.client_id,
            "production" // MEXC doesn't have explicit environment enum yet
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping {id}", id = self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting {id}", id = self.client_id);

        self.cancellation_token.cancel();

        for task in self.tasks.drain(..) {
            task.abort();
        }

        let mut ws = self.ws_client.clone();
        get_runtime().spawn(async move {
            let _ = ws.close().await;
        });

        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing {id}", id = self.client_id);
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        // Reinitialize token in case of reconnection after disconnect
        self.cancellation_token = CancellationToken::new();

        // Request instruments from exchange info
        let instruments = self.request_instruments_internal().await
            .context("failed to request MEXC instruments")?;

        // TODO: Parse and cache instruments once instrument parsing is implemented
        // For now, instruments list is empty until parsing is implemented
        {
            let mut guard = self.instruments.write().expect(MUTEX_POISONED);
            for instrument in &instruments {
                guard.insert(instrument.id(), instrument.clone());
            }
        }

        for instrument in instruments.clone() {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        // Cache instruments in WebSocket client
        for instrument in instruments {
            self.ws_client.cache_instrument(instrument);
        }

        log::info!("Connecting to MEXC WebSocket...");
        self.ws_client.connect(None).await.map_err(|e| {
            log::error!("MEXC WebSocket connection failed: {e:?}");
            anyhow::anyhow!("failed to connect MEXC WebSocket: {e}")
        })?;
        log::info!("MEXC WebSocket connected");

        let stream = self.ws_client.stream();
        let sender = self.data_sender.clone();
        let insts = self.instruments.clone();
        let cancel = self.cancellation_token.clone();

        let handle = get_runtime().spawn(async move {
            pin_mut!(stream);
            loop {
                tokio::select! {
                    Some(message) = stream.next() => {
                        Self::handle_ws_message(message, &sender, &insts);
                    }
                    () = cancel.cancelled() => {
                        log::debug!("WebSocket stream task cancelled");
                        break;
                    }
                }
            }
        });
        self.tasks.push(handle);

        self.is_connected.store(true, Ordering::Release);
        log::info!("Connected: client_id={}", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() {
            return Ok(());
        }

        self.cancellation_token.cancel();

        let _ = self.ws_client.close().await;

        let handles: Vec<_> = self.tasks.drain(..).collect();
        for handle in handles {
            if let Err(e) = handle.await {
                log::error!("Error joining WebSocket task: {e}");
            }
        }

        self.is_connected.store(false, Ordering::Release);
        log::info!("Disconnected: client_id={}", self.client_id);
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    fn subscribe_instruments(&mut self, _cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        log::debug!("subscribe_instruments: MEXC instruments are fetched via HTTP on connect");
        Ok(())
    }

    fn subscribe_instrument(&mut self, _cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        log::debug!("subscribe_instrument: MEXC instruments are fetched via HTTP on connect");
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("MEXC only supports L2_MBP order book deltas");
        }

        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client.clone();
        let symbol = format_mexc_symbol(&instrument_id);

        // MEXC uses incremental depth for order book updates
        let stream = format_mexc_stream(MexcWsChannel::PublicIncreaseDepths.as_str(), &symbol);

        self.spawn_ws(
            async move {
                ws.subscribe(vec![stream])
                    .await
                    .context("book deltas subscription")
            },
            "order book subscription",
        );
        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client.clone();
        let symbol = format_mexc_symbol(&instrument_id);

        // MEXC uses bookTicker for best bid/ask
        let stream = format_mexc_stream(MexcWsChannel::PublicBookTicker.as_str(), &symbol);

        self.spawn_ws(
            async move {
                ws.subscribe(vec![stream])
                    .await
                    .context("quotes subscription")
            },
            "quote subscription",
        );
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client.clone();
        let symbol = format_mexc_symbol(&instrument_id);

        // MEXC uses deals channel for trades
        let stream = format_mexc_stream(MexcWsChannel::PublicDeals.as_str(), &symbol);

        self.spawn_ws(
            async move {
                ws.subscribe(vec![stream])
                    .await
                    .context("trades subscription")
            },
            "trade subscription",
        );
        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let ws = self.ws_client.clone();
        let symbol = format_mexc_symbol(&bar_type.instrument_id());
        let interval = bar_interval_to_mexc_interval(&bar_type.spec())?;

        // MEXC kline stream format: "spot@public.kline.v3.api@BTCUSDT@Min1"
        let stream = format!(
            "{}@{}@{}",
            MexcWsChannel::PublicSpotKline.as_str(),
            symbol,
            interval
        );

        self.spawn_ws(
            async move {
                ws.subscribe(vec![stream])
                    .await
                    .context("bars subscription")
            },
            "bar subscription",
        );
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client.clone();
        let symbol = format_mexc_symbol(&instrument_id);

        let stream = format_mexc_stream(MexcWsChannel::PublicIncreaseDepths.as_str(), &symbol);

        self.spawn_ws(
            async move {
                ws.unsubscribe(vec![stream])
                    .await
                    .context("book deltas unsubscribe")
            },
            "order book unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client.clone();
        let symbol = format_mexc_symbol(&instrument_id);

        let stream = format_mexc_stream(MexcWsChannel::PublicBookTicker.as_str(), &symbol);

        self.spawn_ws(
            async move {
                ws.unsubscribe(vec![stream])
                    .await
                    .context("quotes unsubscribe")
            },
            "quote unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client.clone();
        let symbol = format_mexc_symbol(&instrument_id);

        let stream = format_mexc_stream(MexcWsChannel::PublicDeals.as_str(), &symbol);

        self.spawn_ws(
            async move {
                ws.unsubscribe(vec![stream])
                    .await
                    .context("trades unsubscribe")
            },
            "trade unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let ws = self.ws_client.clone();
        let symbol = format_mexc_symbol(&bar_type.instrument_id());
        let interval = bar_interval_to_mexc_interval(&bar_type.spec())?;

        let stream = format!(
            "{}@{}@{}",
            MexcWsChannel::PublicSpotKline.as_str(),
            symbol,
            interval
        );

        self.spawn_ws(
            async move {
                ws.unsubscribe(vec![stream])
                    .await
                    .context("bars unsubscribe")
            },
            "bar unsubscribe",
        );
        Ok(())
    }

    fn request_instruments(&self, request: &RequestInstruments) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = self.venue();
        let start = request.start;
        let end = request.end;
        let params = request.params.clone();
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http.get_exchange_info(None).await {
                Ok(_mexc_instruments) => {
                    // TODO: Parse instruments properly
                    // For now, we'll return an empty list until instrument parsing is implemented
                    let instruments: Vec<InstrumentAny> = Vec::new();

                    for instrument in &instruments {
                        upsert_instrument(&instruments_cache, instrument.clone());
                    }

                    let response = DataResponse::Instruments(InstrumentsResponse::new(
                        request_id,
                        client_id,
                        venue,
                        instruments,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send instruments response: {e}");
                    }
                }
                Err(e) => log::error!("Instruments request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: &RequestInstrument) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = request.start;
        let end = request.end;
        let params = request.params.clone();
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            {
                let guard = instruments.read().expect(MUTEX_POISONED);
                if let Some(instrument) = guard.get(&instrument_id) {
                    let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                        request_id,
                        client_id,
                        instrument.id(),
                        instrument.clone(),
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    )));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send instrument response: {e}");
                    }
                    return;
                }
            }

            match http.get_exchange_info(None).await {
                Ok(_all_mexc_instruments) => {
                    // TODO: Parse instruments properly and find the requested one
                    // For now, we'll return an error
                    log::error!("Instrument parsing not yet fully implemented");
                }
                Err(e) => log::error!("Instrument request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_trades(&self, _request: &RequestTrades) -> anyhow::Result<()> {
        anyhow::bail!("request_trades not yet implemented for MEXC")
    }

    fn request_bars(&self, _request: &RequestBars) -> anyhow::Result<()> {
        anyhow::bail!("request_bars not yet implemented for MEXC")
    }
}

