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
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports,
            GenerateOrderStatusReportsBuilder, GeneratePositionStatusReports,
            GeneratePositionStatusReportsBuilder, ModifyOrder, QueryAccount, QueryOrder,
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
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderType},
    events::{
        AccountState, OrderAccepted, OrderCanceled, OrderCancelRejected, OrderEventAny,
        OrderFilled, OrderModifyRejected, OrderRejected, OrderSubmitted,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::super::{
    common::enums::MexcOrderStatus,
    config::MexcExecClientConfig,
    http::client::MexcRawHttpClient,
    websocket::{
        client::MexcWebSocketClient,
        handler_exec::MexcExecWsFeedHandler,
        messages::{ExecHandlerCommand, MexcExecWsMessage, NautilusWsMessage},
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
    exec_cmd_tx: Option<tokio::sync::mpsc::UnboundedSender<ExecHandlerCommand>>,
    handler_signal: Arc<AtomicBool>,
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
            exec_cmd_tx: None,
            handler_signal: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Handles WebSocket messages from the user data stream.
    fn handle_ws_message(
        message: NautilusWsMessage,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        trader_id: TraderId,
        account_id: AccountId,
        account_type: AccountType,
        clock: &'static AtomicTime,
    ) {
        match message {
            NautilusWsMessage::Exec(exec_msg) => {
                // Wrap in catch_unwind to prevent panics from crashing the WebSocket task
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Self::handle_exec_message(
                        exec_msg,
                        exec_sender,
                        trader_id,
                        account_id,
                        account_type,
                        clock,
                    );
                }));
                
                if let Err(e) = result {
                    log::error!("Panic in handle_exec_message: {:?}", e);
                    // Don't let the panic crash the WebSocket connection
                }
            }
            NautilusWsMessage::Data(_) => {
                // Data messages are for the data client, ignore here
            }
            NautilusWsMessage::Reconnected => {
                log::warn!("User data stream WebSocket reconnected - this may indicate a connection issue");
            }
        }
    }

    /// Handles execution messages from the user data stream.
    fn handle_exec_message(
        message: MexcExecWsMessage,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        trader_id: TraderId,
        account_id: AccountId,
        account_type: AccountType,
        clock: &'static AtomicTime,
    ) {
        match message {
            MexcExecWsMessage::OrderUpdate { msg, symbol } => {
                Self::handle_order_update(&msg, symbol.as_deref(), exec_sender, trader_id, account_id, clock);
            }
            MexcExecWsMessage::DealUpdate { msg, symbol } => {
                Self::handle_deal_update(&msg, symbol.as_deref(), exec_sender, trader_id, account_id, clock);
            }
            MexcExecWsMessage::AccountUpdate(update) => {
                Self::handle_account_update(&update, exec_sender, account_id, account_type, clock);
            }
        }
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

    /// Refreshes account state by requesting it from the exchange.
    async fn refresh_account_state(&self) -> anyhow::Result<AccountState> {
        use crate::http::query::GetAccountParams;

        let mexc_account = self
            .http_client
            .get_account(GetAccountParams::default())
            .await
            .context("failed to request MEXC account state")?;

        let ts_now = self.clock.get_time_ns();
        let mut balances = Vec::with_capacity(mexc_account.balances.len());

        for balance in &mexc_account.balances {
            let free: f64 = balance.free.parse().unwrap_or(0.0);
            let locked: f64 = balance.locked.parse().unwrap_or(0.0);
            let total = free + locked;

            // Skip zero balances
            if total == 0.0 && locked == 0.0 {
                continue;
            }

            let currency = Currency::from(balance.asset.as_str());
            let account_balance = AccountBalance::new(
                Money::new(total, currency),
                Money::new(locked.max(0.0), currency),
                Money::new(free.max(0.0), currency),
            );
            balances.push(account_balance);
        }

        // Ensure at least one balance exists
        if balances.is_empty() {
            let zero_currency = Currency::USDT();
            let zero_money = Money::new(0.0, zero_currency);
            let zero_balance = AccountBalance::new(zero_money, zero_money, zero_money);
            balances.push(zero_balance);
        }

        Ok(AccountState::new(
            self.core.account_id,
            self.core.account_type,
            balances,
            Vec::new(), // margins (not applicable for spot)
            true,       // reported
            UUID4::new(),
            ts_now,
            ts_now,
            None, // base currency
        ))
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

    /// Converts MEXC order status (i32) to MexcOrderStatus.
    fn parse_order_status(status: i32) -> MexcOrderStatus {
        // MEXC order status values (from API documentation):
        // 1 = NEW, 2 = FILLED, 3 = PARTIALLY_FILLED, 4 = CANCELED,
        // 5 = PARTIALLY_CANCELED, 6 = REJECTED, 7 = EXPIRED
        match status {
            1 => MexcOrderStatus::New,
            2 => MexcOrderStatus::Filled,
            3 => MexcOrderStatus::PartiallyFilled,
            4 => MexcOrderStatus::Canceled,
            5 => MexcOrderStatus::PartiallyCanceled,
            6 => MexcOrderStatus::Rejected,
            7 => MexcOrderStatus::Expired,
            _ => {
                log::warn!("Unknown MEXC order status: {status}, defaulting to New");
                MexcOrderStatus::New
            }
        }
    }

    /// Converts MEXC trade type (i32) to OrderSide.
    /// MEXC trade_type: 1 = buy, 2 = sell
    fn parse_trade_type(trade_type: i32) -> OrderSide {
        match trade_type {
            1 => OrderSide::Buy,
            2 => OrderSide::Sell,
            _ => {
                log::warn!("Unknown MEXC trade type: {trade_type}, defaulting to Buy");
                OrderSide::Buy
            }
        }
    }

    /// Registers an order with the execution handler for context tracking.
    fn register_order(&self, order: &nautilus_model::orders::OrderAny) {
        if let Some(ref cmd_tx) = self.exec_cmd_tx {
            let cmd = ExecHandlerCommand::RegisterOrder {
                client_order_id: order.client_order_id(),
                trader_id: order.trader_id(),
                strategy_id: order.strategy_id(),
                instrument_id: order.instrument_id(),
            };
            if let Err(e) = cmd_tx.send(cmd) {
                log::error!("Failed to register order with handler: {e}");
            }
        }
    }

    /// Registers a cancel request with the execution handler for context tracking.
    fn register_cancel(
        &self,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
    ) {
        if let Some(ref cmd_tx) = self.exec_cmd_tx {
            let cmd = ExecHandlerCommand::RegisterCancel {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                venue_order_id,
            };
            if let Err(e) = cmd_tx.send(cmd) {
                log::error!("Failed to register cancel with handler: {e}");
            }
        }
    }

    /// Handles ORDER_UPDATE events from the user data stream.
    fn handle_order_update(
        msg: &crate::proto::PrivateOrdersV3Api,
        symbol: Option<&str>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        trader_id: TraderId,
        account_id: AccountId,
        clock: &'static AtomicTime,
    ) {

        // Parse order status
        let mexc_status = Self::parse_order_status(msg.status);
        let status: nautilus_model::enums::OrderStatus = mexc_status.into();

        // Convert timestamps (MEXC uses milliseconds)
        let ts_event = UnixNanos::from((msg.create_time * 1_000_000) as u64);
        let ts_init = clock.get_time_ns();

        // Parse instrument ID from symbol (prefer wrapper symbol, fallback to message fields)
        let symbol = symbol
            .or_else(|| msg.market.as_deref())
            .or_else(|| msg.symbol_id.as_deref())
            .unwrap_or("UNKNOWN");
        
        // Try to get instrument from cache, or use default precision
        let _price_precision = 8_u8; // Default precision
        let _size_precision = 8_u8; // Default precision
        let instrument_id = InstrumentId::from(format!("{}.MEXC", symbol).as_str());

        // Handle empty client_id - use order id as fallback
        let client_order_id = if msg.client_id.is_empty() {
            log::debug!("Empty client_id in order update, using order id as fallback: {}", msg.id);
            ClientOrderId::new(&msg.id)
        } else {
            ClientOrderId::new(&msg.client_id)
        };
        let venue_order_id = VenueOrderId::new(msg.id.clone());

        // For external orders we don't have strategy_id, use EXTERNAL
        let strategy_id = StrategyId::new("EXTERNAL");

        // Parse quantities and prices
        let _quantity: f64 = msg.quantity.parse().unwrap_or(0.0);
        let _price: f64 = msg.price.parse().unwrap_or(0.0);
        let remain_quantity: f64 = msg.remain_quantity.parse().unwrap_or(0.0);

        match status {
            nautilus_model::enums::OrderStatus::Accepted => {
                // Send OrderAccepted event even for external orders (not in cache)
                // Execution engine will log a warning but won't crash
                // This matches Binance's behavior
                let event = OrderAccepted::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    account_id,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                );

                if let Err(e) =
                    exec_sender.send(ExecutionEvent::Order(OrderEventAny::Accepted(event)))
                {
                    log::warn!("Failed to send OrderAccepted event: {e}");
                }
            }
            nautilus_model::enums::OrderStatus::Canceled => {
                // Send OrderCanceled event even for external orders (not in cache)
                // Execution engine will log a warning but won't crash
                let event = OrderCanceled::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                );

                if let Err(e) =
                    exec_sender.send(ExecutionEvent::Order(OrderEventAny::Canceled(event)))
                {
                    log::warn!("Failed to send OrderCanceled event: {e}");
                }
            }
            nautilus_model::enums::OrderStatus::Rejected => {
                let event = OrderRejected::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    account_id,
                    "Order rejected by exchange".into(),
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    false,
                );

                if let Err(e) =
                    exec_sender.send(ExecutionEvent::Order(OrderEventAny::Rejected(event)))
                {
                    log::warn!("Failed to send OrderRejected event: {e}");
                }
            }
            nautilus_model::enums::OrderStatus::Expired => {
                let event = OrderCanceled::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                );

                if let Err(e) =
                    exec_sender.send(ExecutionEvent::Order(OrderEventAny::Canceled(event)))
                {
                    log::warn!("Failed to send OrderCanceled (expired) event: {e}");
                }
            }
            _ => {
                // Partially filled or other status - will be handled by deal updates
                log::debug!(
                    "Order status update: client_order_id={}, status={:?}, remain_quantity={}",
                    client_order_id,
                    status,
                    remain_quantity
                );
            }
        }
    }

    /// Handles DEAL_UPDATE (trade fill) events from the user data stream.
    fn handle_deal_update(
        msg: &crate::proto::PrivateDealsV3Api,
        symbol: Option<&str>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        trader_id: TraderId,
        account_id: AccountId,
        clock: &'static AtomicTime,
    ) {
        // Convert timestamps (MEXC uses milliseconds)
        let ts_event = UnixNanos::from((msg.time * 1_000_000) as u64);
        let ts_init = clock.get_time_ns();

        // Parse instrument ID from symbol (use wrapper symbol, fallback to UNKNOWN)
        let symbol = symbol.unwrap_or("UNKNOWN");
        let instrument_id = InstrumentId::from(format!("{}.MEXC", symbol).as_str());

        // Handle empty client_order_id - use order_id as fallback
        let client_order_id = if msg.client_order_id.is_empty() {
            log::warn!("Empty client_order_id in deal update, using order_id as fallback: {}", msg.order_id);
            ClientOrderId::new(&msg.order_id)
        } else {
            ClientOrderId::new(&msg.client_order_id)
        };
        let venue_order_id = VenueOrderId::new(msg.order_id.clone());

        // For external orders we don't have strategy_id, use EXTERNAL
        let strategy_id = StrategyId::new("EXTERNAL");

        // Parse quantities and prices
        let quantity: f64 = msg.quantity.parse().unwrap_or(0.0);
        let price: f64 = msg.price.parse().unwrap_or(0.0);
        let commission: f64 = msg.fee_amount.parse().unwrap_or(0.0);

        // Use default precision since we don't have cache access
        let (price_precision, size_precision) = (8_u8, 8_u8);

        let commission_currency = Currency::from(msg.fee_currency.as_str());

        let liquidity_side = if msg.is_maker {
            LiquiditySide::Maker
        } else {
            LiquiditySide::Taker
        };

        let order_side = Self::parse_trade_type(msg.trade_type);

        let event = OrderFilled::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            TradeId::new(&msg.trade_id),
            order_side,
            OrderType::Limit, // MEXC doesn't specify order type in deal message
            Quantity::new(quantity, size_precision),
            Price::new(price, price_precision),
            commission_currency,
            liquidity_side,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            None,
            Some(Money::new(commission, commission_currency)),
        );

        if let Err(e) = exec_sender.send(ExecutionEvent::Order(OrderEventAny::Filled(event))) {
            log::warn!("Failed to send OrderFilled event: {e}");
        }
    }

    /// Handles ACCOUNT_UPDATE events from the user data stream.
    fn handle_account_update(
        msg: &crate::proto::PrivateAccountV3Api,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        account_id: AccountId,
        account_type: AccountType,
        clock: &'static AtomicTime,
    ) {
        // Convert timestamps (MEXC uses milliseconds)
        let ts_event = UnixNanos::from((msg.time * 1_000_000) as u64);

        // Parse balance amounts
        // MEXC balance_amount is the available balance, frozen_amount is the locked balance
        // Total = available + frozen
        let available: f64 = msg.balance_amount.parse().unwrap_or(0.0);
        let frozen: f64 = msg.frozen_amount.parse().unwrap_or(0.0);
        let total = available + frozen;

        if total == 0.0 && frozen == 0.0 {
            // Skip zero balances
            return;
        }

        let currency = Currency::from(msg.vcoin_name.as_str());

        let balances = vec![AccountBalance::new(
            Money::new(total, currency),
            Money::new(frozen.max(0.0), currency),
            Money::new(available.max(0.0), currency),
        )];

        let account_state = AccountState::new(
            account_id,
            account_type,
            balances,
            Vec::new(), // margins (not applicable for spot)
            true,       // reported
            UUID4::new(),
            ts_event,
            clock.get_time_ns(),
            None, // base currency
        );

        if let Err(e) = exec_sender.send(ExecutionEvent::Account(account_state)) {
            log::warn!("Failed to send account state update: {e}");
        }
    }

    /// Internal method to submit an order.
    fn submit_order_internal(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        use crate::common::enums::{MexcOrderType, MexcSide, MexcTimeInForce};
        use crate::http::query::PostOrderParams;

        let http_client = self.http_client.clone();
        let order = self.core.get_order(&cmd.client_order_id)?;

        // Register order with handler for context tracking before HTTP request
        self.register_order(&order);

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
        let _mexc_tif = MexcTimeInForce::try_from_time_in_force(time_in_force)?;

        let symbol = Self::format_mexc_symbol(&instrument_id);
        let _size_precision = self.get_instrument_precision(instrument_id).1;

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
        
        let params = PostOrderParams {
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

        // Load instruments if not already done
        if !self.instruments_initialized.load(Ordering::Acquire) {
            let mexc_instruments = self
                .http_client
                .get_exchange_info(None)
                .await
                .context("failed to request MEXC exchange info")?;

            let ts_init = self.clock.get_time_ns();
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

            if instruments.is_empty() {
                log::warn!("No instruments returned for MEXC");
            } else {
                log::info!("Loaded {} MEXC instruments", instruments.len());

                // Add instruments to Nautilus Cache for reconciliation
                let cache = self.core.cache();
                for instrument in &instruments {
                    if let Err(e) = cache.borrow_mut().add_instrument(instrument.clone()) {
                        log::debug!("Instrument already in cache: {e}");
                    }
                }
            }

            self.instruments_initialized.store(true, Ordering::Release);
        }

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

            // Subscribe to private execution messages (orders, deals, account updates)
            // MEXC requires explicit subscription to private channels
            let private_topics = vec![
                "spot@private.orders.v3.api.pb".to_string(),
                "spot@private.deals.v3.api.pb".to_string(),
                "spot@private.account.v3.api.pb".to_string(),
            ];
            
            log::info!("Subscribing to private execution channels: {:?}", private_topics);
            if let Err(e) = ws_client.subscribe(private_topics).await {
                log::warn!("Failed to subscribe to private execution channels: {e}");
                // Don't fail connection if subscription fails, as messages might still come through
            } else {
                log::info!("Successfully subscribed to private execution channels");
            }

            // Create channels for the execution handler
            let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
            let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();

            // Store command channel for order registration
            self.exec_cmd_tx = Some(cmd_tx.clone());

            // Create channel for account updates (handled separately from order events)
            let (account_tx, mut account_rx) = tokio::sync::mpsc::unbounded_channel();
            let exec_sender_account = self.exec_sender.clone();
            let cancel_account = self.cancellation_token.clone();
            let account_handle_task = get_runtime().spawn(async move {
                loop {
                    tokio::select! {
                        Some(account_state) = account_rx.recv() => {
                            if let Err(e) = exec_sender_account.send(ExecutionEvent::Account(account_state)) {
                                log::error!("Failed to send account state event: {e}");
                                break;
                            }
                        }
                        () = cancel_account.cancelled() => {
                            log::debug!("Account update handler task cancelled");
                            break;
                        }
                    }
                }
            });

            // Create and initialize the execution handler
            let mut handler = MexcExecWsFeedHandler::new(
                self.clock,
                self.core.trader_id,
                self.core.account_id,
                self.core.account_type,
                self.handler_signal.clone(),
                cmd_rx,
                raw_rx,
                Some(account_tx),
            );

            // Set up raw message forwarding from WebSocket to handler
            let stream = ws_client.stream();
            let cancel = self.cancellation_token.clone();
            let raw_forward_task = get_runtime().spawn(async move {
                pin_mut!(stream);
                loop {
                    tokio::select! {
                        Some(message) = stream.next() => {
                            if let Err(e) = raw_tx.send(message) {
                                log::error!("Failed to forward raw message to handler: {e}");
                                break;
                            }
                        }
                        () = cancel.cancelled() => {
                            log::debug!("Raw message forwarding task cancelled");
                            break;
                        }
                    }
                }
            });

            // Start handler processing loop
            let exec_sender = self.exec_sender.clone();
            let cancel = self.cancellation_token.clone();
            let handler_task = get_runtime().spawn(async move {
                loop {
                    tokio::select! {
                        Some(event) = handler.next() => {
                            if let Err(e) = exec_sender.send(ExecutionEvent::Order(event)) {
                                log::error!("Failed to send order event from handler: {e}");
                                break;
                            }
                        }
                        () = cancel.cancelled() => {
                            log::debug!("Handler processing task cancelled");
                            break;
                        }
                    }
                }
            });

            // Store all tasks
            let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
            tasks.push(raw_forward_task);
            tasks.push(handler_task);
            tasks.push(account_handle_task);
            drop(tasks);

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

        // Request initial account state
        let account_state = self
            .refresh_account_state()
            .await
            .context("failed to request MEXC account state")?;

        if !account_state.balances.is_empty() {
            log::info!(
                "Received account state with {} balance(s)",
                account_state.balances.len()
            );
        }

        if let Err(e) = self
            .exec_sender
            .send(ExecutionEvent::Account(account_state))
        {
            log::warn!("Failed to send account state: {e}");
        }

        // Wait for account to be registered in cache before completing connect
        self.await_account_registered(30.0).await?;

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
            let _order = order;
            let client_order_id = _order.client_order_id();
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

        let Some(_order) = order else {
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
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let Some(instrument_id) = cmd.instrument_id else {
            log::warn!("generate_order_status_report requires instrument_id: {cmd:?}");
            return Ok(None);
        };

        use crate::http::query::GetOrderParams;

        let symbol = Self::format_mexc_symbol(&instrument_id);
        let mut params = GetOrderParams::default();
        params.symbol = symbol;

        if let Some(venue_order_id) = &cmd.venue_order_id {
            params.order_id = Some(venue_order_id.to_string());
        }
        if let Some(client_order_id) = &cmd.client_order_id {
            params.orig_client_order_id = Some(client_order_id.to_string());
        }

        let order = self.http_client.get_order(params).await?;
        let (price_precision, size_precision) = self.get_instrument_precision(instrument_id);
        let report = order.to_order_status_report(self.core.account_id, instrument_id, price_precision, size_precision)?;

        Ok(Some(report))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        use crate::http::query::GetOpenOrdersParams;

        let mut reports = Vec::new();

        if cmd.open_only {
            // Get all open orders (or for specific symbol if provided)
            let symbol = cmd.instrument_id.map(|id| Self::format_mexc_symbol(&id));
            let params = if let Some(s) = symbol {
                Some(GetOpenOrdersParams { symbol: Some(s) })
            } else {
                Some(GetOpenOrdersParams { symbol: None })
            };

            let orders = self.http_client.get_open_orders(params).await?;

            for order in orders {
                // Try to find instrument from cache or use provided instrument_id
                let instrument_id = if let Some(cmd_instrument_id) = cmd.instrument_id {
                    cmd_instrument_id
                } else {
                    // Try to find instrument from cache by symbol
                    let cache = self.core.cache().borrow();
                    cache
                        .instruments(&self.venue(), None)
                        .into_iter()
                        .find(|i| i.symbol().as_str() == order.symbol.as_str())
                        .map(|i| i.id())
                        .unwrap_or_else(|| {
                            // Fallback: construct instrument ID from symbol
                            InstrumentId::from(format!("{}.MEXC", order.symbol).as_str())
                        })
                };

                let (price_precision, size_precision) = self.get_instrument_precision(instrument_id);
                match order.to_order_status_report(self.core.account_id, instrument_id, price_precision, size_precision) {
                    Ok(report) => reports.push(report),
                    Err(e) => {
                        log::warn!("Failed to convert MEXC order to status report: {e}");
                    }
                }
            }
        } else {
            // For historical orders, MEXC doesn't have a direct API endpoint
            // We can only query open orders, so return empty for now
            log::debug!("MEXC doesn't support querying historical orders via API");
        }

        Ok(reports)
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
        // MEXC Spot trading doesn't have positions in the traditional sense
        // Returns empty for spot (could be extended for margin positions if needed)
        Ok(Vec::new())
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");

        let ts_now = self.clock.get_time_ns();

        let start = lookback_mins.map(|mins| {
            let lookback_ns = mins * 60 * 1_000_000_000;
            UnixNanos::from(ts_now.as_u64().saturating_sub(lookback_ns))
        });

        // Use open_only=true to get all open orders across instruments
        let order_cmd = GenerateOrderStatusReportsBuilder::default()
            .ts_init(ts_now)
            .open_only(true)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let position_cmd = GeneratePositionStatusReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let (order_reports, position_reports) = tokio::try_join!(
            self.generate_order_status_reports(&order_cmd),
            self.generate_position_status_reports(&position_cmd),
        )?;

        log::info!("Received {} OrderStatusReports", order_reports.len());
        log::info!("Received {} PositionReports", position_reports.len());

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            self.venue(),
            ts_now,
            None,
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }
}

