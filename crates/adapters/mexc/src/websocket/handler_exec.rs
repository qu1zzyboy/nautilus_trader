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

//! MEXC execution WebSocket handler.
//!
//! Implements the two-tier architecture with pending order maps for correlating
//! WebSocket order updates with the original order context (strategy_id, etc.).

use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_core::{nanos::UnixNanos, time::AtomicTime};
use nautilus_model::{
    enums::{AccountType, LiquiditySide, OrderSide, OrderType},
    events::{AccountState, OrderAccepted, OrderCanceled, OrderFilled, OrderRejected},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId,
    },
    types::{Currency, Money, Price, Quantity},
};

use super::messages::{ExecHandlerCommand, MexcExecWsMessage, NautilusWsMessage};

/// Data cached for pending place requests to correlate with responses.
pub type PlaceRequestData = (ClientOrderId, TraderId, StrategyId, InstrumentId);

/// Data cached for pending cancel requests to correlate with responses.
pub type CancelRequestData = (
    ClientOrderId,
    TraderId,
    StrategyId,
    InstrumentId,
    Option<VenueOrderId>,
);

/// MEXC execution WebSocket handler.
///
/// Processes user data stream messages and maintains pending order state
/// to correlate WebSocket updates with the original order context.
pub struct MexcExecWsFeedHandler {
    clock: &'static AtomicTime,
    trader_id: TraderId,
    account_id: AccountId,
    account_type: AccountType,
    signal: Arc<AtomicBool>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<ExecHandlerCommand>,
    msg_rx: tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>,
    account_update_sender: Option<tokio::sync::mpsc::UnboundedSender<AccountState>>,
    pending_place_requests: AHashMap<ClientOrderId, PlaceRequestData>,
    pending_cancel_requests: AHashMap<ClientOrderId, CancelRequestData>,
    active_orders: AHashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId)>,
    message_queue: VecDeque<nautilus_model::events::OrderEventAny>,
}

impl Debug for MexcExecWsFeedHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(MexcExecWsFeedHandler))
            .field("trader_id", &self.trader_id)
            .field("account_id", &self.account_id)
            .field("pending_place_requests", &self.pending_place_requests.len())
            .field(
                "pending_cancel_requests",
                &self.pending_cancel_requests.len(),
            )
            .field("active_orders", &self.active_orders.len())
            .finish_non_exhaustive()
    }
}

impl MexcExecWsFeedHandler {
    /// Creates a new [`MexcExecWsFeedHandler`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        clock: &'static AtomicTime,
        trader_id: TraderId,
        account_id: AccountId,
        account_type: AccountType,
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<ExecHandlerCommand>,
        msg_rx: tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>,
        account_update_sender: Option<tokio::sync::mpsc::UnboundedSender<AccountState>>,
    ) -> Self {
        Self {
            clock,
            trader_id,
            account_id,
            account_type,
            signal,
            cmd_rx,
            msg_rx,
            account_update_sender,
            pending_place_requests: AHashMap::new(),
            pending_cancel_requests: AHashMap::new(),
            active_orders: AHashMap::new(),
            message_queue: VecDeque::new(),
        }
    }

    /// Processes commands and messages, returning the next output event.
    pub async fn next(&mut self) -> Option<nautilus_model::events::OrderEventAny> {
        loop {
            if self.signal.load(Ordering::Relaxed) {
                return None;
            }

            // Return queued messages first
            if let Some(msg) = self.message_queue.pop_front() {
                return Some(msg);
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd);
                }
                Some(msg) = self.msg_rx.recv() => {
                    if let Some(event) = self.handle_message(msg) {
                        return Some(event);
                    }
                }
                else => {
                    return None;
                }
            }
        }
    }

    fn handle_command(&mut self, cmd: ExecHandlerCommand) {
        match cmd {
            ExecHandlerCommand::RegisterOrder {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            } => {
                let data = (client_order_id, trader_id, strategy_id, instrument_id);
                self.pending_place_requests.insert(client_order_id, data);
                self.active_orders
                    .insert(client_order_id, (trader_id, strategy_id, instrument_id));
            }
            ExecHandlerCommand::RegisterCancel {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                venue_order_id,
            } => {
                let data = (
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    venue_order_id,
                );
                self.pending_cancel_requests.insert(client_order_id, data);
            }
        }
    }

    fn handle_message(&mut self, msg: NautilusWsMessage) -> Option<nautilus_model::events::OrderEventAny> {
        match msg {
            NautilusWsMessage::Exec(exec_msg) => self.handle_exec_message(exec_msg),
            NautilusWsMessage::Reconnected => {
                log::warn!("WebSocket reconnected - subscriptions should be restored");
                None
            }
            NautilusWsMessage::Data(_) => {
                // Data messages are for the data client, ignore here
                None
            }
        }
    }

    fn handle_exec_message(&mut self, msg: MexcExecWsMessage) -> Option<nautilus_model::events::OrderEventAny> {
        match msg {
            MexcExecWsMessage::OrderUpdate { msg, symbol } => {
                self.handle_order_update(&msg, symbol.as_deref())
            }
            MexcExecWsMessage::DealUpdate { msg, symbol } => {
                self.handle_deal_update(&msg, symbol.as_deref())
            }
            MexcExecWsMessage::AccountUpdate(_update) => {
                // Account updates are handled by the old handle_account_update method
                // which is still called from handle_ws_message
                // This handler only processes order-related events
                None
            }
        }
    }

    fn handle_order_update(
        &mut self,
        msg: &crate::proto::PrivateOrdersV3Api,
        symbol: Option<&str>,
    ) -> Option<nautilus_model::events::OrderEventAny> {
        let ts_event = UnixNanos::from((msg.create_time * 1_000_000) as u64);
        let ts_init = self.clock.get_time_ns();

        let symbol = symbol
            .or_else(|| msg.market.as_deref())
            .or_else(|| msg.symbol_id.as_deref())
            .unwrap_or("UNKNOWN");
        let instrument_id = InstrumentId::from(format!("{}.MEXC", symbol).as_str());

        let client_order_id = if msg.client_id.is_empty() {
            log::debug!("Empty client_id in order update, using order id as fallback: {}", msg.id);
            ClientOrderId::new(&msg.id)
        } else {
            ClientOrderId::new(&msg.client_id)
        };
        let venue_order_id = VenueOrderId::new(msg.id.clone());

        // Look up order context from pending/active maps, falling back to EXTERNAL
        let (trader_id, strategy_id, _instrument_id) =
            self.get_order_context(&client_order_id, symbol);

        // Parse order status (MEXC status: 1=NEW, 2=FILLED, 3=PARTIALLY_FILLED, 4=CANCELED, 5=PARTIALLY_CANCELED, 6=EXPIRED)
        let status = match msg.status {
            1 => nautilus_model::enums::OrderStatus::Accepted,
            2 => nautilus_model::enums::OrderStatus::Filled,
            3 => nautilus_model::enums::OrderStatus::PartiallyFilled,
            4 | 5 => nautilus_model::enums::OrderStatus::Canceled,
            6 => nautilus_model::enums::OrderStatus::Expired,
            _ => {
                log::warn!("Unknown MEXC order status: {}, defaulting to Accepted", msg.status);
                nautilus_model::enums::OrderStatus::Accepted
            }
        };

        match status {
            nautilus_model::enums::OrderStatus::Accepted => {
                // Move from pending to active on acceptance
                self.pending_place_requests.remove(&client_order_id);

                let event = OrderAccepted::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    self.account_id,
                    nautilus_core::UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                );

                Some(nautilus_model::events::OrderEventAny::Accepted(event))
            }
            nautilus_model::enums::OrderStatus::Canceled => {
                self.pending_cancel_requests.remove(&client_order_id);
                self.active_orders.remove(&client_order_id);

                let event = OrderCanceled::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    nautilus_core::UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(self.account_id),
                );

                Some(nautilus_model::events::OrderEventAny::Canceled(event))
            }
            nautilus_model::enums::OrderStatus::Rejected => {
                self.pending_place_requests.remove(&client_order_id);
                self.active_orders.remove(&client_order_id);

                let event = OrderRejected::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    self.account_id,
                    "Order rejected by exchange".into(),
                    nautilus_core::UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    false,
                );

                Some(nautilus_model::events::OrderEventAny::Rejected(event))
            }
            nautilus_model::enums::OrderStatus::Expired => {
                self.pending_cancel_requests.remove(&client_order_id);
                self.active_orders.remove(&client_order_id);

                let event = OrderCanceled::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    nautilus_core::UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(self.account_id),
                );

                Some(nautilus_model::events::OrderEventAny::Canceled(event))
            }
            _ => {
                // Partially filled or other status - will be handled by deal updates
                log::debug!(
                    "Order status update: client_order_id={}, status={:?}",
                    client_order_id,
                    status
                );
                None
            }
        }
    }

    fn handle_deal_update(
        &mut self,
        msg: &crate::proto::PrivateDealsV3Api,
        symbol: Option<&str>,
    ) -> Option<nautilus_model::events::OrderEventAny> {
        let ts_event = UnixNanos::from((msg.time * 1_000_000) as u64);
        let ts_init = self.clock.get_time_ns();

        let symbol = symbol.unwrap_or("UNKNOWN");
        let instrument_id = InstrumentId::from(format!("{}.MEXC", symbol).as_str());

        let client_order_id = if msg.client_order_id.is_empty() {
            log::warn!("Empty client_order_id in deal update, using order_id as fallback: {}", msg.order_id);
            ClientOrderId::new(&msg.order_id)
        } else {
            ClientOrderId::new(&msg.client_order_id)
        };
        let venue_order_id = VenueOrderId::new(msg.order_id.clone());

        // Look up order context from pending/active maps, falling back to EXTERNAL
        let (trader_id, strategy_id, _instrument_id) =
            self.get_order_context(&client_order_id, symbol);

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

        // Parse trade type (MEXC trade_type: 1 = buy, 2 = sell)
        let order_side = match msg.trade_type {
            1 => OrderSide::Buy,
            2 => OrderSide::Sell,
            _ => {
                log::warn!("Unknown MEXC trade type: {}, defaulting to Buy", msg.trade_type);
                OrderSide::Buy
            }
        };

        let event = OrderFilled::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            self.account_id,
            TradeId::new(&msg.trade_id),
            order_side,
            OrderType::Limit, // MEXC doesn't specify order type in deal message
            Quantity::new(quantity, size_precision),
            Price::new(price, price_precision),
            commission_currency,
            liquidity_side,
            nautilus_core::UUID4::new(),
            ts_event,
            ts_init,
            false,
            None,
            Some(Money::new(commission, commission_currency)),
        );

        Some(nautilus_model::events::OrderEventAny::Filled(event))
    }

    /// Gets the order context (trader_id, strategy_id, instrument_id) for a given client_order_id.
    ///
    /// First checks pending place requests, then active orders, and finally
    /// constructs instrument ID from the symbol using the configured product type.
    fn get_order_context(
        &self,
        client_order_id: &ClientOrderId,
        symbol: &str,
    ) -> (TraderId, StrategyId, InstrumentId) {
        // First check pending place requests
        if let Some((_, trader_id, strategy_id, instrument_id)) =
            self.pending_place_requests.get(client_order_id)
        {
            return (*trader_id, *strategy_id, *instrument_id);
        }

        // Then check active orders
        if let Some((trader_id, strategy_id, instrument_id)) =
            self.active_orders.get(client_order_id)
        {
            return (*trader_id, *strategy_id, *instrument_id);
        }

        // Fall back to EXTERNAL for untracked orders (restart, external creation)
        let instrument_id = InstrumentId::from(format!("{}.MEXC", symbol).as_str());

        log::debug!(
            "Order context not found for {client_order_id}, using EXTERNAL with {instrument_id}"
        );
        (self.trader_id, StrategyId::new("EXTERNAL"), instrument_id)
    }
}

