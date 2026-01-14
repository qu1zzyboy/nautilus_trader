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

//! MEXC-specific enumerations shared by HTTP and WebSocket components.

use nautilus_model::enums::{OrderSide, OrderStatus, OrderType, TimeInForce};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

use crate::error::MexcError;

/// Represents the side of an order or trade (Buy/Sell).
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MexcSide {
    /// Buy side of a trade or order.
    #[serde(rename = "BUY", alias = "Buy", alias = "buy")]
    Buy,
    /// Sell side of a trade or order.
    #[serde(rename = "SELL", alias = "Sell", alias = "sell")]
    Sell,
}

impl TryFrom<OrderSide> for MexcSide {
    type Error = MexcError;

    fn try_from(value: OrderSide) -> Result<Self, Self::Error> {
        match value {
            OrderSide::Buy => Ok(Self::Buy),
            OrderSide::Sell => Ok(Self::Sell),
            _ => Err(MexcError::InvalidOrderSide(format!("Invalid order side: {value:?}"))),
        }
    }
}

impl MexcSide {
    /// Try to convert from Nautilus OrderSide.
    ///
    /// # Errors
    ///
    /// Returns an error if the order side is not Buy or Sell.
    pub fn try_from_order_side(value: OrderSide) -> anyhow::Result<Self> {
        Self::try_from(value).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

impl From<MexcSide> for OrderSide {
    fn from(side: MexcSide) -> Self {
        match side {
            MexcSide::Buy => Self::Buy,
            MexcSide::Sell => Self::Sell,
        }
    }
}

/// Represents the available order types on MEXC.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MexcOrderType {
    /// Market order, executed immediately at current market price.
    #[serde(rename = "MARKET")]
    Market,
    /// Limit order, executed only at specified price or better.
    #[serde(rename = "LIMIT")]
    Limit,
    /// Limit maker order, ensures the order is added to the order book as a maker.
    #[serde(rename = "LIMIT_MAKER")]
    LimitMaker,
    /// Immediate or Cancel order, fills immediately or cancels remaining quantity.
    #[serde(rename = "IMMEDIATE_OR_CANCEL")]
    ImmediateOrCancel,
    /// Fill or Kill order, must fill completely immediately or cancel.
    #[serde(rename = "FILL_OR_KILL")]
    FillOrKill,
}

impl TryFrom<OrderType> for MexcOrderType {
    type Error = MexcError;

    fn try_from(value: OrderType) -> Result<Self, Self::Error> {
        match value {
            OrderType::Market => Ok(Self::Market),
            OrderType::Limit => Ok(Self::Limit),
            OrderType::StopMarket => Err(MexcError::InvalidOrderType(
                "StopMarket order type is not directly supported by MEXC".to_string(),
            )),
            OrderType::StopLimit => Err(MexcError::InvalidOrderType(
                "StopLimit order type is not directly supported by MEXC".to_string(),
            )),
            OrderType::MarketToLimit => Ok(Self::Market),
            OrderType::MarketIfTouched => Err(MexcError::InvalidOrderType(
                "MarketIfTouched order type is not supported by MEXC".to_string(),
            )),
            OrderType::LimitIfTouched => Err(MexcError::InvalidOrderType(
                "LimitIfTouched order type is not supported by MEXC".to_string(),
            )),
            OrderType::TrailingStopMarket => Err(MexcError::InvalidOrderType(
                "TrailingStopMarket order type is not supported by MEXC".to_string(),
            )),
            OrderType::TrailingStopLimit => Err(MexcError::InvalidOrderType(
                "TrailingStopLimit order type is not supported by MEXC".to_string(),
            )),
        }
    }
}

impl MexcOrderType {
    /// Try to convert from Nautilus OrderType with anyhow::Result.
    ///
    /// # Errors
    ///
    /// Returns an error if the order type is not supported by MEXC.
    pub fn try_from_order_type(value: OrderType) -> anyhow::Result<Self> {
        Self::try_from(value).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

impl From<MexcOrderType> for OrderType {
    fn from(value: MexcOrderType) -> Self {
        match value {
            MexcOrderType::Market => Self::Market,
            MexcOrderType::Limit => Self::Limit,
            MexcOrderType::LimitMaker => Self::Limit,
            MexcOrderType::ImmediateOrCancel => Self::Market,
            MexcOrderType::FillOrKill => Self::Market,
        }
    }
}

/// Represents the possible states of an order throughout its lifecycle.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
    pub enum MexcOrderStatus {
    /// Order has been placed but not yet filled.
    #[serde(rename = "NEW")]
    New,
    /// Order has been completely filled.
    #[serde(rename = "FILLED")]
    Filled,
    /// Order has been partially filled.
    #[serde(rename = "PARTIALLY_FILLED")]
    PartiallyFilled,
    /// Order has been canceled by user or system.
    #[serde(rename = "CANCELED")]
    Canceled,
    /// Order has been partially canceled.
    #[serde(rename = "PARTIALLY_CANCELED")]
    PartiallyCanceled,
    /// Order was rejected by the system.
    #[serde(rename = "REJECTED")]
    Rejected,
    /// Order has expired.
    #[serde(rename = "EXPIRED")]
    Expired,
}

impl From<MexcOrderStatus> for OrderStatus {
    fn from(value: MexcOrderStatus) -> Self {
        match value {
            MexcOrderStatus::New => Self::Accepted,
            MexcOrderStatus::PartiallyFilled => Self::PartiallyFilled,
            MexcOrderStatus::Filled => Self::Filled,
            MexcOrderStatus::Canceled => Self::Canceled,
            MexcOrderStatus::PartiallyCanceled => Self::Canceled,
            MexcOrderStatus::Rejected => Self::Rejected,
            MexcOrderStatus::Expired => Self::Expired,
        }
    }
}

impl TryFrom<OrderStatus> for MexcOrderStatus {
    type Error = MexcError;

    fn try_from(value: OrderStatus) -> Result<Self, Self::Error> {
        match value {
            OrderStatus::Accepted => Ok(Self::New),
            OrderStatus::PartiallyFilled => Ok(Self::PartiallyFilled),
            OrderStatus::Filled => Ok(Self::Filled),
            OrderStatus::Canceled => Ok(Self::Canceled),
            OrderStatus::Rejected => Ok(Self::Rejected),
            OrderStatus::Expired => Ok(Self::Expired),
            _ => Err(MexcError::InvalidOrderStatus(format!(
                "Cannot convert OrderStatus {value:?} to MexcOrderStatus"
            ))),
        }
    }
}

/// Specifies how long an order should remain active.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MexcTimeInForce {
    /// Good Till Cancel - order remains active until canceled.
    #[serde(rename = "GTC")]
    Gtc,
    /// Immediate Or Cancel - fill immediately, cancel remainder.
    #[serde(rename = "IOC")]
    Ioc,
    /// Fill Or Kill - must fill completely immediately or cancel.
    #[serde(rename = "FOK")]
    Fok,
}

impl TryFrom<MexcTimeInForce> for TimeInForce {
    type Error = MexcError;

    fn try_from(value: MexcTimeInForce) -> Result<Self, Self::Error> {
        match value {
            MexcTimeInForce::Gtc => Ok(Self::Gtc),
            MexcTimeInForce::Ioc => Ok(Self::Ioc),
            MexcTimeInForce::Fok => Ok(Self::Fok),
        }
    }
}

impl TryFrom<TimeInForce> for MexcTimeInForce {
    type Error = MexcError;

    fn try_from(value: TimeInForce) -> Result<Self, Self::Error> {
        match value {
            TimeInForce::Gtc => Ok(Self::Gtc),
            TimeInForce::Ioc => Ok(Self::Ioc),
            TimeInForce::Fok => Ok(Self::Fok),
            _ => Err(MexcError::InvalidTimeInForce(format!(
                "TimeInForce {value:?} is not supported by MEXC"
            ))),
        }
    }
}

impl MexcTimeInForce {
    /// Try to convert from Nautilus TimeInForce with anyhow::Result.
    ///
    /// # Errors
    ///
    /// Returns an error if the time in force is not supported by MEXC.
    pub fn try_from_time_in_force(value: TimeInForce) -> anyhow::Result<Self> {
        Self::try_from(value).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

/// Represents MEXC kline/candlestick intervals.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum MexcKlineInterval {
    /// 1 minute interval.
    #[serde(rename = "1m")]
    #[strum(serialize = "1m")]
    M1,
    /// 5 minutes interval.
    #[serde(rename = "5m")]
    #[strum(serialize = "5m")]
    M5,
    /// 15 minutes interval.
    #[serde(rename = "15m")]
    #[strum(serialize = "15m")]
    M15,
    /// 30 minutes interval.
    #[serde(rename = "30m")]
    #[strum(serialize = "30m")]
    M30,
    /// 60 minutes (1 hour) interval.
    #[serde(rename = "60m", alias = "1h")]
    #[strum(serialize = "60m")]
    H1,
    /// 4 hours interval.
    #[serde(rename = "4h")]
    #[strum(serialize = "4h")]
    H4,
    /// 1 day interval.
    #[serde(rename = "1d")]
    #[strum(serialize = "1d")]
    D1,
    /// 1 week interval.
    #[serde(rename = "1W")]
    #[strum(serialize = "1W")]
    W1,
    /// 1 month interval.
    #[serde(rename = "1M")]
    #[strum(serialize = "1M")]
    Month1,
}

/// Represents MEXC symbol status.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MexcSymbolStatus {
    /// Symbol is trading.
    Trading,
    /// Symbol is halted.
    Halt,
    /// Symbol is break.
    Break,
}

/// MEXC WebSocket operation types.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, PartialEq)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum MexcWsOperation {
    /// Subscribe to a channel.
    Subscribe,
    /// Unsubscribe from a channel.
    Unsubscribe,
}

/// MEXC WebSocket topic/channel types.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, PartialEq)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum MexcWsTopic {
    /// Market depth (order book) updates.
    Depth,
    /// Trade updates.
    Trade,
    /// K-line (candlestick) updates.
    Kline,
    /// Ticker updates.
    Ticker,
    /// 24-hour ticker statistics.
    Ticker24Hr,
}
