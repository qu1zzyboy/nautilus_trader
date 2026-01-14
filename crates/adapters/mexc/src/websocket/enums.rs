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

//! WebSocket-specific enumerations for MEXC adapter.

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString};

pub use crate::common::enums::{MexcWsOperation, MexcWsTopic};

/// MEXC WebSocket channel identifiers.
///
/// These correspond to the channel field in MEXC's protobuf messages.
/// Channel format: "spot@public.{type}.v3.api" or "spot@private.{type}.v3.api"
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MexcWsChannel {
    /// Public trades/deals channel.
    /// Channel string: "spot@public.deals.v3.api"
    #[serde(rename = "spot@public.deals.v3.api")]
    #[strum(serialize = "spot@public.deals.v3.api")]
    PublicDeals,
    /// Public aggregate deals channel.
    /// Channel string: "spot@public.aggreDeals.v3.api"
    #[serde(rename = "spot@public.aggreDeals.v3.api")]
    #[strum(serialize = "spot@public.aggreDeals.v3.api")]
    PublicAggreDeals,
    /// Public incremental depth (order book updates) channel.
    /// Channel string: "spot@public.increase.depth.v3.api"
    #[serde(rename = "spot@public.increase.depth.v3.api")]
    #[strum(serialize = "spot@public.increase.depth.v3.api")]
    PublicIncreaseDepths,
    /// Public limit depth (full order book snapshot) channel.
    /// Channel string: "spot@public.limit.depth.v3.api"
    #[serde(rename = "spot@public.limit.depth.v3.api")]
    #[strum(serialize = "spot@public.limit.depth.v3.api")]
    PublicLimitDepths,
    /// Public aggregate depth channel.
    /// Channel string: "spot@public.aggreDepth.v3.api"
    #[serde(rename = "spot@public.aggreDepth.v3.api")]
    #[strum(serialize = "spot@public.aggreDepth.v3.api")]
    PublicAggreDepths,
    /// Public book ticker (best bid/ask) channel.
    /// Channel string: "spot@public.bookTicker.v3.api"
    #[serde(rename = "spot@public.bookTicker.v3.api")]
    #[strum(serialize = "spot@public.bookTicker.v3.api")]
    PublicBookTicker,
    /// Public aggregate book ticker channel.
    /// Channel string: "spot@public.aggreBookTicker.v3.api"
    #[serde(rename = "spot@public.aggreBookTicker.v3.api")]
    #[strum(serialize = "spot@public.aggreBookTicker.v3.api")]
    PublicAggreBookTicker,
    /// Public spot kline (candlestick) channel.
    /// Channel string: "spot@public.kline.v3.api"
    #[serde(rename = "spot@public.kline.v3.api")]
    #[strum(serialize = "spot@public.kline.v3.api")]
    PublicSpotKline,
    /// Public mini ticker channel.
    /// Channel string: "spot@public.miniTicker.v3.api"
    #[serde(rename = "spot@public.miniTicker.v3.api")]
    #[strum(serialize = "spot@public.miniTicker.v3.api")]
    PublicMiniTicker,
    /// Public mini tickers (batch) channel.
    /// Channel string: "spot@public.miniTickers.v3.api"
    #[serde(rename = "spot@public.miniTickers.v3.api")]
    #[strum(serialize = "spot@public.miniTickers.v3.api")]
    PublicMiniTickers,
    /// Private orders channel (requires authentication).
    /// Channel string: "spot@private.orders.v3.api"
    #[serde(rename = "spot@private.orders.v3.api")]
    #[strum(serialize = "spot@private.orders.v3.api")]
    PrivateOrders,
    /// Private deals (executions) channel (requires authentication).
    /// Channel string: "spot@private.deals.v3.api"
    #[serde(rename = "spot@private.deals.v3.api")]
    #[strum(serialize = "spot@private.deals.v3.api")]
    PrivateDeals,
    /// Private account updates channel (requires authentication).
    /// Channel string: "spot@private.account.v3.api"
    #[serde(rename = "spot@private.account.v3.api")]
    #[strum(serialize = "spot@private.account.v3.api")]
    PrivateAccount,
    /// Unknown/unrecognized channel type (default when field is missing).
    #[default]
    #[serde(other)]
    #[strum(to_string = "unknown")]
    Unknown,
}

impl MexcWsChannel {
    /// Returns `true` if this is a private channel requiring authentication.
    #[must_use]
    pub const fn is_private(&self) -> bool {
        matches!(
            self,
            Self::PrivateOrders | Self::PrivateDeals | Self::PrivateAccount
        )
    }

    /// Returns `true` if this is a public channel.
    #[must_use]
    pub const fn is_public(&self) -> bool {
        !self.is_private()
    }

    /// Returns `true` if this is an unknown/unrecognized channel type.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// Returns the channel string as used in MEXC WebSocket API.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PublicDeals => "spot@public.deals.v3.api",
            Self::PublicAggreDeals => "spot@public.aggreDeals.v3.api",
            Self::PublicIncreaseDepths => "spot@public.increase.depth.v3.api",
            Self::PublicLimitDepths => "spot@public.limit.depth.v3.api",
            Self::PublicAggreDepths => "spot@public.aggreDepth.v3.api",
            Self::PublicBookTicker => "spot@public.bookTicker.v3.api",
            Self::PublicAggreBookTicker => "spot@public.aggreBookTicker.v3.api",
            Self::PublicSpotKline => "spot@public.kline.v3.api",
            Self::PublicMiniTicker => "spot@public.miniTicker.v3.api",
            Self::PublicMiniTickers => "spot@public.miniTickers.v3.api",
            Self::PrivateOrders => "spot@private.orders.v3.api",
            Self::PrivateDeals => "spot@private.deals.v3.api",
            Self::PrivateAccount => "spot@private.account.v3.api",
            Self::Unknown => "unknown",
        }
    }

    /// Parses a channel string into a `MexcWsChannel` enum.
    ///
    /// # Errors
    ///
    /// Returns `None` if the channel string is not recognized.
    #[must_use]
    pub fn from_str(channel: &str) -> Option<Self> {
        match channel {
            "spot@public.deals.v3.api" => Some(Self::PublicDeals),
            "spot@public.aggreDeals.v3.api" => Some(Self::PublicAggreDeals),
            "spot@public.increase.depth.v3.api" => Some(Self::PublicIncreaseDepths),
            "spot@public.limit.depth.v3.api" => Some(Self::PublicLimitDepths),
            "spot@public.aggreDepth.v3.api" => Some(Self::PublicAggreDepths),
            "spot@public.bookTicker.v3.api" => Some(Self::PublicBookTicker),
            "spot@public.aggreBookTicker.v3.api" => Some(Self::PublicAggreBookTicker),
            "spot@public.kline.v3.api" => Some(Self::PublicSpotKline),
            "spot@public.miniTicker.v3.api" => Some(Self::PublicMiniTicker),
            "spot@public.miniTickers.v3.api" => Some(Self::PublicMiniTickers),
            "spot@private.orders.v3.api" => Some(Self::PrivateOrders),
            "spot@private.deals.v3.api" => Some(Self::PrivateDeals),
            "spot@private.account.v3.api" => Some(Self::PrivateAccount),
            _ => None,
        }
    }
}

/// WebSocket message types for MEXC subscription confirmations.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MexcWsMessageType {
    /// Subscription confirmed.
    #[default]
    Subscribed,
    /// Unsubscription confirmed.
    Unsubscribed,
    /// Error message.
    Error,
    /// Unknown/unrecognized message type.
    #[serde(other)]
    #[strum(to_string = "unknown")]
    Unknown,
}
