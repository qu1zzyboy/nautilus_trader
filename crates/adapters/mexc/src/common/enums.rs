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

//! Enumerations for MEXC adapter.

use strum::{Display, EnumString};

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

