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

//! Live execution client implementation for the MEXC adapter.

use std::{
    future::Future,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::ExecutionClient,
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    messages::{
        ExecutionEvent, ExecutionReport as NautilusExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports,
            GeneratePositionStatusReports, ModifyOrder, QueryAccount, QueryOrder,
            SubmitOrder, SubmitOrderList,
        },
    },
};
use nautilus_core::{
    MUTEX_POISONED, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderSide, OrderType, TimeInForce},
    events::{
        OrderCancelRejected, OrderEventAny, OrderModifyRejected, OrderRejected, OrderSubmitted,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::Instrument,
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{Price, Quantity},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::super::{
    config::MexcExecClientConfig,
    http::client::MexcRawHttpClient,
    websocket::{
        client::MexcWebSocketClient,
        messages::NautilusWsMessage,
    },
};
use crate::common::consts::MEXC_VENUE;

/// Listen key keepalive interval (30 minutes).
const LISTEN_KEY_KEEPALIVE_SECS: u64 = 30 * 60;

/// Live execution client for MEXC trading.
///
/// Implements the [`ExecutionClient`] trait for order management on MEXC.
/// Uses HTTP API for order operations and WebSocket for real-time order updates
/// via user data stream.
#[derive(Debug)]
pub struct MexcExecutionClient {
    clock: &'static AtomicTime,
    core: ExecutionClientCore,
    config: MexcExecClientConfig,
    http_client: MexcRawHttpClient,
    ws_client: Option<MexcWebSocketClient>,
    exec_sender: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
    listen_key: Arc<RwLock<Option<String>>>,
    cancellation_token: CancellationToken,
    ws_task: Mutex<Option<JoinHandle<()>>>,
    keepalive_task: Mutex<Option<JoinHandle<()>>>,
    started: bool,
    connected: AtomicBool,
    instruments_initialized: AtomicBool,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl MexcExecutionClient {
    /// Creates a new [`MexcExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client fails to initialize or credentials are missing.
    pub fn new(core: ExecutionClientCore, config: MexcExecClientConfig) -> anyhow::Result<Self> {
        let api_key = config.api_key.clone();
        let api_secret = config.api_secret.clone();

        if api_key.is_none() || api_secret.is_none() {
            anyhow::bail!("API key and secret are required for MEXC execution client");
        }

        let http_client = MexcRawHttpClient::with_credentials(
            api_key.clone().unwrap(),
            api_secret.clone().unwrap(),
            config.base_url_http.clone().unwrap_or_else(|| crate::common::consts::MEXC_HTTP_URL.to_string()),
            config.http_timeout_secs,
            config.max_retries,
            None, // retry_delay_ms
            None, // retry_delay_max_ms
            None, // max_requests_per_second
            None, // max_requests_per_minute
            config.http_proxy_url.clone(),
        )
        .context("failed to construct MEXC HTTP client")?;

        let ws_client = MexcWebSocketClient::new(
            config.base_url_ws.clone(),
            api_key,
            api_secret,
            Some(config.account_id),
            config.heartbeat_interval_secs,
        )
        .context("failed to construct MEXC WebSocket client")?;

        let clock = get_atomic_clock_realtime();
        let exec_sender = get_exec_event_sender();

        Ok(Self {
            clock,
            core,
            config,
            http_client,
            ws_client: Some(ws_client),
            exec_sender,
            listen_key: Arc::new(RwLock::new(None)),
            cancellation_token: CancellationToken::new(),
            ws_task: Mutex::new(None),
            keepalive_task: Mutex::new(None),
            started: false,
            connected: AtomicBool::new(false),
            instruments_initialized: AtomicBool::new(false),
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    /// Handles WebSocket messages from the user data stream.
    fn handle_ws_message(
        _message: NautilusWsMessage,
        _exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        _account_id: AccountId,
        _account_type: AccountType,
        _clock: &'static AtomicTime,
    ) {
        // TODO: Implement message handling for order updates, account updates, etc.
        // This will parse MEXC WebSocket messages and convert them to Nautilus events
    }

    async fn await_account_registered(&self, timeout_secs: f64) -> anyhow::Result<()> {
        let timeout = Duration::from_secs_f64(timeout_secs);
        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            if self.core.get_account().is_some() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        anyhow::bail!("Account registration timeout after {timeout_secs} seconds");
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let runtime = get_runtime();
        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        let handle = runtime.spawn(async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e}");
            }
        });
        tasks.push(handle);
    }

    fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    /// Returns a mutable reference to the WebSocket client (for testing/subscription).
    pub fn ws_client_mut(&mut self) -> Option<&mut MexcWebSocketClient> {
        self.ws_client.as_mut()
    }

    /// Gets instrument precision from cache.
    fn get_instrument_precision(&self, instrument_id: InstrumentId) -> (u8, u8) {
        let cache = self.core.cache().borrow();
        cache
            .instrument(&instrument_id)
            .map_or((8, 8), |i| (i.price_precision(), i.size_precision()))
    }

    /// Formats instrument ID to MEXC symbol format.
    fn format_mexc_symbol(instrument_id: &InstrumentId) -> String {
        instrument_id.symbol.inner().to_string()
    }

    /// Internal method to submit an order.
    fn submit_order_internal(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        use crate::common::enums::{MexcOrderType, MexcSide, MexcTimeInForce};
        use crate::http::query::PostOrderParams;

        let http_client = self.http_client.clone();
        let order = self.core.get_order(&cmd.client_order_id)?;
        let exec_sender = self.exec_sender.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let ts_init = cmd.ts_init;
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let order_side = order.order_side();
        let order_type = order.order_type();
        let quantity = order.quantity();
        let time_in_force = order.time_in_force();
        let price = order.price();
        let clock = self.clock;

        // Convert to MEXC types
        let mexc_side = MexcSide::try_from_order_side(order_side)?;
        let mexc_order_type = MexcOrderType::try_from_order_type(order_type)?;
        let mexc_tif = MexcTimeInForce::try_from_time_in_force(time_in_force)?;

        let symbol = Self::format_mexc_symbol(&instrument_id);
        let (_, size_precision) = self.get_instrument_precision(instrument_id);

        // Build order parameters
        // MEXC requires uppercase strings: BUY/SELL, MARKET/LIMIT
        // Note: MEXC API doesn't have timeInForce parameter
        let side_str = match mexc_side {
            crate::common::enums::MexcSide::Buy => "BUY",
            crate::common::enums::MexcSide::Sell => "SELL",
        };
        let order_type_str = match mexc_order_type {
            crate::common::enums::MexcOrderType::Market => "MARKET",
            crate::common::enums::MexcOrderType::Limit => "LIMIT",
            crate::common::enums::MexcOrderType::LimitMaker => "LIMIT_MAKER",
            crate::common::enums::MexcOrderType::ImmediateOrCancel => "IMMEDIATE_OR_CANCEL",
            crate::common::enums::MexcOrderType::FillOrKill => "FILL_OR_KILL",
        };
        
        let mut params = PostOrderParams {
            symbol,
            side: side_str.to_string(),
            order_type: order_type_str.to_string(),
            quantity: Some(quantity.to_string()),
            price: price.map(|p| p.to_string()),
            new_client_order_id: Some(client_order_id.to_string()),
            time_in_force: None, // MEXC API doesn't support timeInForce parameter
            stop_price: None, // MEXC spot doesn't support stop orders
        };

        // HTTP only generates OrderRejected on failure.
        // OrderAccepted comes from WebSocket user data stream.
        self.spawn_task("submit_order", async move {
            let result = http_client.place_order(params).await;

            match result {
                Ok(mexc_order) => {
                    log::debug!(
                        "Order submit accepted: client_order_id={}, venue_order_id={:?}",
                        client_order_id,
                        mexc_order.order_id
                    );
                    // OrderAccepted will come from WebSocket stream
                }
                Err(e) => {
                    let rejected_event = OrderRejected::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        account_id,
                        format!("submit-order-error: {e}").into(),
                        UUID4::new(),
                        ts_init,
                        clock.get_time_ns(),
                        false,
                        false,
                    );

                    if let Err(e) = exec_sender.send(ExecutionEvent::Order(
                        OrderEventAny::Rejected(rejected_event),
                    )) {
                        log::warn!("Failed to send OrderRejected event: {e}");
                    }

                    return Err(anyhow::anyhow!("{e}"));
                }
            }

            Ok(())
        });

        Ok(())
    }

    /// Internal method to cancel an order.
    fn cancel_order_internal(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        use crate::http::query::DeleteOrderParams;

        let http_client = self.http_client.clone();
        let command = cmd.clone();
        let exec_sender = self.exec_sender.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let ts_init = cmd.ts_init;
        let instrument_id = command.instrument_id;
        let venue_order_id = command.venue_order_id;
        let client_order_id = Some(command.client_order_id);
        let clock = self.clock;

        let symbol = Self::format_mexc_symbol(&instrument_id);

        // HTTP only generates OrderCancelRejected on failure.
        // OrderCanceled comes from WebSocket user data stream.
        self.spawn_task("cancel_order", async move {
            let params = DeleteOrderParams {
                symbol,
                order_id: venue_order_id.as_ref().map(|id| id.to_string()),
                orig_client_order_id: client_order_id.as_ref().map(|id| id.to_string()),
            };

            let result = http_client.cancel_order(params).await;

            match result {
                Ok(_mexc_order) => {
                    log::debug!(
                        "Cancel request accepted: client_order_id={}, venue_order_id={:?}",
                        command.client_order_id,
                        venue_order_id
                    );
                    // OrderCanceled will come from WebSocket stream
                }
                Err(e) => {
                    let rejected_event = OrderCancelRejected::new(
                        trader_id,
                        command.strategy_id,
                        command.instrument_id,
                        command.client_order_id,
                        format!("cancel-order-error: {e}").into(),
                        UUID4::new(),
                        clock.get_time_ns(),
                        ts_init,
                        false,
                        command.venue_order_id,
                        Some(account_id),
                    );

                    if let Err(e) = exec_sender.send(ExecutionEvent::Order(
                        OrderEventAny::CancelRejected(rejected_event),
                    )) {
                        log::warn!("Failed to send OrderCancelRejected event: {e}");
                    }

                    return Err(anyhow::anyhow!("{e}"));
                }
            }

            Ok(())
        });

        Ok(())
    }
}

#[async_trait(?Send)]
impl ExecutionClient for MexcExecutionClient {
    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        *MEXC_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.get_account()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.connected.load(Ordering::Acquire) {
            return Ok(());
        }

        // Reinitialize cancellation token in case of reconnection
        self.cancellation_token = CancellationToken::new();

        // Create listen key for user data stream
        log::info!("Creating listen key for MEXC user data stream...");
        let listen_key_response = self
            .http_client
            .create_listen_key()
            .await
            .context("failed to create listen key")?;
        let listen_key = listen_key_response.listen_key;
        log::info!("Listen key created successfully");

        {
            let mut key_guard = self.listen_key.write().expect(MUTEX_POISONED);
            *key_guard = Some(listen_key.clone());
        }

        // Connect WebSocket with listenkey (MEXC uses listenkey as URL parameter)
        if let Some(ref mut ws_client) = self.ws_client {
            log::info!("Connecting to MEXC user data stream WebSocket...");
            ws_client
                .connect(Some(&listen_key))
                .await
                .map_err(|e| {
                    log::error!("MEXC WebSocket connection failed: {e:?}");
                    anyhow::anyhow!("failed to connect MEXC WebSocket: {e}")
                })?;
            log::info!("MEXC WebSocket connected");

            // Start WebSocket message processing loop
            let stream = ws_client.stream();
            let exec_sender = self.exec_sender.clone();
            let account_id = self.core.account_id;
            let account_type = self.core.account_type;
            let clock = self.clock;
            let cancel = self.cancellation_token.clone();

            let ws_task = get_runtime().spawn(async move {
                pin_mut!(stream);
                loop {
                    tokio::select! {
                        Some(message) = stream.next() => {
                            Self::handle_ws_message(
                                message,
                                &exec_sender,
                                account_id,
                                account_type,
                                clock,
                            );
                        }
                        () = cancel.cancelled() => {
                            log::debug!("User data stream task cancelled");
                            break;
                        }
                    }
                }
            });
            *self.ws_task.lock().expect(MUTEX_POISONED) = Some(ws_task);

            // Start listen key keepalive task
            let http_client = self.http_client.clone();
            let listen_key_ref = self.listen_key.clone();
            let cancel = self.cancellation_token.clone();

            let keepalive_task = get_runtime().spawn(async move {
                let mut interval =
                    tokio::time::interval(Duration::from_secs(LISTEN_KEY_KEEPALIVE_SECS));
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let key = {
                                let guard = listen_key_ref.read().expect(MUTEX_POISONED);
                                guard.clone()
                            };
                            if let Some(ref key) = key {
                                match http_client.keepalive_listen_key(key).await {
                                    Ok(()) => {
                                        log::debug!("Listen key keepalive sent successfully");
                                    }
                                    Err(e) => {
                                        log::warn!("Listen key keepalive failed: {e}");
                                    }
                                }
                            }
                        }
                        () = cancel.cancelled() => {
                            log::debug!("Listen key keepalive task cancelled");
                            break;
                        }
                    }
                }
            });
            *self.keepalive_task.lock().expect(MUTEX_POISONED) = Some(keepalive_task);
        }

        // TODO: Request initial account state
        // let account_state = self.refresh_account_state().await?;

        self.connected.store(true, Ordering::Release);
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.connected.load(Ordering::Acquire) {
            return Ok(());
        }

        // Cancel all background tasks
        self.cancellation_token.cancel();

        // Wait for WebSocket task to complete
        let ws_task = self.ws_task.lock().expect(MUTEX_POISONED).take();
        if let Some(task) = ws_task {
            let _ = task.await;
        }

        // Wait for keepalive task to complete
        let keepalive_task = self.keepalive_task.lock().expect(MUTEX_POISONED).take();
        if let Some(task) = keepalive_task {
            let _ = task.await;
        }

        // Close WebSocket
        if let Some(ref mut ws_client) = self.ws_client {
            let _ = ws_client.close().await;
        }

        // Close listen key
        let listen_key = self.listen_key.read().expect(MUTEX_POISONED).clone();
        if let Some(ref key) = listen_key {
            if let Err(e) = self.http_client.close_listen_key(key).await {
                log::warn!("Failed to close listen key: {e}");
            }
        }
        *self.listen_key.write().expect(MUTEX_POISONED) = None;

        self.connected.store(false, Ordering::Release);
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        // TODO: Implement account query
        Ok(())
    }

    fn query_order(&self, _cmd: &QueryOrder) -> anyhow::Result<()> {
        // TODO: Implement order query
        Ok(())
    }

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;

        if order.is_closed() {
            let client_order_id = order.client_order_id();
            log::warn!("Cannot submit closed order {client_order_id}");
            return Ok(());
        }

        let event = OrderSubmitted::new(
            self.core.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            self.core.account_id,
            UUID4::new(),
            cmd.ts_init,
            self.clock.get_time_ns(),
        );

        log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
        if let Err(e) = self
            .exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::Submitted(event)))
        {
            log::warn!("Failed to send OrderSubmitted event: {e}");
        }

        self.submit_order_internal(cmd)
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        log::warn!(
            "submit_order_list not yet implemented for MEXC (got {} orders)",
            cmd.order_list.orders.len()
        );
        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        // MEXC doesn't support order modification directly.
        // We need to cancel the existing order and submit a new one.
        log::warn!(
            "MEXC doesn't support order modification. Canceling order {} and submitting new order",
            cmd.client_order_id
        );

        let order = {
            let cache = self.core.cache().borrow();
            cache.order(&cmd.client_order_id).cloned()
        };

        let Some(order) = order else {
            log::warn!(
                "Cannot modify order {}: not found in cache",
                cmd.client_order_id
            );
            let rejected_event = OrderModifyRejected::new(
                self.core.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                "Order not found in cache for modify".into(),
                UUID4::new(),
                self.clock.get_time_ns(),
                cmd.ts_init,
                false,
                cmd.venue_order_id,
                Some(self.core.account_id),
            );

            if let Err(e) = self.exec_sender.send(ExecutionEvent::Order(
                OrderEventAny::ModifyRejected(rejected_event),
            )) {
                log::warn!("Failed to send OrderModifyRejected event: {e}");
            }
            return Ok(());
        };

        // Cancel the existing order first
        let cancel_cmd = CancelOrder {
            trader_id: self.core.trader_id,
            strategy_id: cmd.strategy_id,
            instrument_id: cmd.instrument_id,
            client_order_id: cmd.client_order_id,
            venue_order_id: cmd.venue_order_id,
            command_id: UUID4::new(),
            ts_init: cmd.ts_init,
            client_id: Some(self.core.client_id),
            params: None,
        };

        self.cancel_order(&cancel_cmd)?;

        // TODO: Submit new order with modified parameters
        // For now, just log a warning
        log::warn!(
            "Order modification requires canceling and resubmitting. New order submission not yet implemented."
        );

        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        self.cancel_order_internal(cmd)
    }

    fn cancel_all_orders(&self, _cmd: &CancelAllOrders) -> anyhow::Result<()> {
        // TODO: Implement cancel all orders
        Ok(())
    }

    fn batch_cancel_orders(&self, _cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        // TODO: Implement batch cancel orders
        Ok(())
    }

    fn generate_account_state(
        &self,
        balances: Vec<nautilus_model::types::AccountBalance>,
        margins: Vec<nautilus_model::types::MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        self.core
            .generate_account_state(balances, margins, reported, ts_event)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.started {
            return Ok(());
        }

        self.started = true;
        log::info!(
            "Started: client_id={}, account_id={}, account_type={:?}",
            self.core.client_id,
            self.core.account_id,
            self.core.account_type,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if !self.started {
            return Ok(());
        }

        self.started = false;
        self.connected.store(false, Ordering::Release);
        self.abort_pending_tasks();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        // TODO: Implement order status report generation
        Ok(None)
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        // TODO: Implement batch order status reports
        Ok(Vec::new())
    }

    async fn generate_fill_reports(
        &self,
        _cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        // TODO: Implement fill reports generation
        Ok(Vec::new())
    }

    async fn generate_position_status_reports(
        &self,
        _cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        // TODO: Implement position status reports
        Ok(Vec::new())
    }

    async fn generate_mass_status(
        &self,
        _lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        // TODO: Implement mass status generation
        Ok(None)
    }
}

