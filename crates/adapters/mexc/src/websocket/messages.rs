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

//! WebSocket message types for MEXC adapter.

use nautilus_model::data::Data;
use serde::{Deserialize, Serialize};

/// MEXC WebSocket subscription request message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MexcSubscription {
    /// The operation type (subscribe/unsubscribe).
    pub method: String,
    /// The subscription parameters.
    pub param: MexcSubscriptionParam,
}

/// MEXC WebSocket subscription parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MexcSubscriptionParam {
    /// The symbol to subscribe to.
    pub symbol: String,
}

/// MEXC WebSocket subscription response message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MexcSubscriptionResponse {
    /// Response status code (200 for success).
    #[serde(default)]
    pub status: Option<u64>,
    /// Response message.
    #[serde(default)]
    pub msg: Option<String>,
    /// The method that was called.
    #[serde(default)]
    pub method: Option<String>,
    /// The subscription parameters.
    #[serde(default)]
    pub param: Option<MexcSubscriptionParam>,
}

/// Internal WebSocket message type for MEXC.
#[derive(Clone, Debug)]
pub enum MexcWsMessage {
    /// Reconnection signal.
    Reconnected,
    /// Subscription confirmation or error.
    Subscription {
        /// Whether the subscription was successful.
        success: bool,
        /// The topic that was subscribed/unsubscribed.
        topic: Option<String>,
        /// Error message if subscription failed.
        error: Option<String>,
    },
    /// Market data message.
    Data(Vec<Data>),
    /// Execution message (order updates, account updates, etc.).
    Exec(MexcExecWsMessage),
}

/// Execution-related WebSocket messages for MEXC.
#[derive(Clone, Debug)]
pub enum MexcExecWsMessage {
    /// Order update message.
    OrderUpdate {
        msg: crate::proto::PrivateOrdersV3Api,
        symbol: Option<String>,
    },
    /// Trade/deal update message.
    DealUpdate {
        msg: crate::proto::PrivateDealsV3Api,
        symbol: Option<String>,
    },
    /// Account update message.
    AccountUpdate(crate::proto::PrivateAccountV3Api),
}

/// Nautilus WebSocket message wrapper.
///
/// This enum contains fully-parsed Nautilus domain objects ready for consumption
/// by the Python layer without additional processing.
#[derive(Clone, Debug)]
pub enum NautilusWsMessage {
    /// Reconnection signal.
    Reconnected,
    /// Market data (trades, quotes, bars, order book deltas).
    Data(Vec<Data>),
    /// Execution messages (order updates, account updates, etc.).
    Exec(MexcExecWsMessage),
}

/// Commands for the MEXC execution WebSocket handler.
///
/// These commands allow the execution client to register orders and manage
/// the handler's internal state for correlating WebSocket updates with order context.
#[derive(Clone, Debug)]
pub enum ExecHandlerCommand {
    /// Register an order for context tracking.
    RegisterOrder {
        client_order_id: nautilus_model::identifiers::ClientOrderId,
        trader_id: nautilus_model::identifiers::TraderId,
        strategy_id: nautilus_model::identifiers::StrategyId,
        instrument_id: nautilus_model::identifiers::InstrumentId,
    },
    /// Register a cancel request for context tracking.
    RegisterCancel {
        client_order_id: nautilus_model::identifiers::ClientOrderId,
        trader_id: nautilus_model::identifiers::TraderId,
        strategy_id: nautilus_model::identifiers::StrategyId,
        instrument_id: nautilus_model::identifiers::InstrumentId,
        venue_order_id: Option<nautilus_model::identifiers::VenueOrderId>,
    },
}
