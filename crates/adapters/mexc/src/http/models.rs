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

//! Data structures representing MEXC REST API payloads.

use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// MEXC instrument information.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcInstrument {
    /// Trading symbol.
    pub symbol: Ustr,
    /// Base currency.
    pub base_currency: Option<Ustr>,
    /// Quote currency.
    pub quote_currency: Option<Ustr>,
    /// Price precision (number of decimal places).
    pub price_precision: Option<u8>,
    /// Quantity precision (number of decimal places).
    pub quantity_precision: Option<u8>,
    /// Minimum order quantity.
    pub min_quantity: Option<String>,
    /// Maximum order quantity.
    pub max_quantity: Option<String>,
    /// Minimum order amount (in quote currency).
    pub min_amount: Option<String>,
    /// Tick size (minimum price increment).
    pub tick_size: Option<String>,
    /// Trading status.
    pub status: Option<String>,
}

/// MEXC trade data.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcTrade {
    /// Trade ID.
    pub id: Option<String>,
    /// Trading symbol.
    pub symbol: Ustr,
    /// Trade price.
    pub price: String,
    /// Trade quantity.
    pub quantity: String,
    /// Trade timestamp.
    pub time: Option<i64>,
    /// Trade direction (BUY/SELL).
    pub is_buyer_maker: Option<bool>,
}

/// MEXC order book entry.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MexcOrderBookEntry {
    /// Price level.
    pub price: String,
    /// Quantity at this price level.
    pub quantity: String,
}

/// MEXC order book snapshot.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcOrderBook {
    /// Last update ID.
    pub last_update_id: Option<u64>,
    /// Bids (buy orders).
    pub bids: Vec<MexcOrderBookEntry>,
    /// Asks (sell orders).
    pub asks: Vec<MexcOrderBookEntry>,
}

/// MEXC order information.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcOrder {
    /// Order ID.
    pub order_id: Option<String>,
    /// Client order ID.
    pub client_order_id: Option<String>,
    /// Trading symbol.
    pub symbol: Ustr,
    /// Order side (BUY/SELL).
    pub side: String,
    /// Order type (LIMIT/MARKET).
    pub order_type: Option<String>,
    /// Order status.
    pub status: Option<String>,
    /// Order price.
    pub price: Option<String>,
    /// Order quantity.
    pub quantity: String,
    /// Filled quantity.
    pub executed_quantity: Option<String>,
    /// Order creation time.
    pub create_time: Option<i64>,
    /// Order update time.
    pub update_time: Option<i64>,
}

/// MEXC account balance.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcBalance {
    /// Asset name.
    pub asset: Ustr,
    /// Available balance.
    pub free: String,
    /// Locked balance.
    pub locked: String,
}

/// MEXC account information.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcAccount {
    /// Account balances.
    pub balances: Vec<MexcBalance>,
    /// Account permissions.
    pub permissions: Option<Vec<String>>,
}

/// MEXC kline/candlestick data.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MexcKline {
    /// Open time.
    pub open_time: i64,
    /// Open price.
    pub open: String,
    /// High price.
    pub high: String,
    /// Low price.
    pub low: String,
    /// Close price.
    pub close: String,
    /// Volume.
    pub volume: String,
    /// Close time.
    pub close_time: i64,
    /// Quote asset volume.
    pub quote_volume: Option<String>,
    /// Number of trades.
    pub trades: Option<u64>,
}

/// MEXC ticker information.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcTicker {
    /// Trading symbol.
    pub symbol: Ustr,
    /// Last price.
    pub last_price: Option<String>,
    /// 24h price change.
    pub price_change_24h: Option<String>,
    /// 24h price change percent.
    pub price_change_percent_24h: Option<String>,
    /// 24h high price.
    pub high_24h: Option<String>,
    /// 24h low price.
    pub low_24h: Option<String>,
    /// 24h volume.
    pub volume_24h: Option<String>,
    /// 24h quote volume.
    pub quote_volume_24h: Option<String>,
}

