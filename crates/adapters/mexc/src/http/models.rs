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

use std::str::FromStr;

use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    reports::OrderStatusReport,
    types::{Price, Quantity},
};
use serde::{Deserialize, Serialize};
use ustr::Ustr;
use uuid::Uuid;

/// MEXC exchange info response wrapper.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MexcExchangeInfo {
    /// List of trading symbols.
    pub symbols: Vec<MexcInstrument>,
}

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
    #[serde(rename = "orderId")]
    pub order_id: Option<String>,
    /// Client order ID.
    #[serde(rename = "clientOrderId")]
    pub client_order_id: Option<String>,
    /// Trading symbol.
    pub symbol: Ustr,
    /// Order side (BUY/SELL).
    pub side: String,
    /// Order type (LIMIT/MARKET).
    #[serde(rename = "type")]
    pub order_type: Option<String>,
    /// Order status.
    pub status: Option<String>,
    /// Order price.
    pub price: Option<String>,
    /// Order quantity (MEXC API returns as "origQty").
    #[serde(rename = "origQty", default)]
    pub quantity: String,
    /// Filled quantity (MEXC API returns as "executedQty").
    #[serde(rename = "executedQty")]
    pub executed_quantity: Option<String>,
    /// Order creation time (MEXC API returns as "transactTime").
    #[serde(rename = "transactTime")]
    pub create_time: Option<i64>,
    /// Order update time.
    pub update_time: Option<i64>,
}

impl MexcOrder {
    /// Converts a MEXC order to a Nautilus OrderStatusReport.
    ///
    /// # Errors
    ///
    /// Returns an error if the order data cannot be parsed or converted.
    pub fn to_order_status_report(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        price_precision: u8,
        size_precision: u8,
    ) -> anyhow::Result<OrderStatusReport> {
        use nautilus_core::time::get_atomic_clock_realtime;

        let ts_now = get_atomic_clock_realtime().get_time_ns();
        let ts_event = self
            .update_time
            .or(self.create_time)
            .map_or(ts_now, |t| UnixNanos::from((t as u64) * 1_000_000_000));

        let client_order_id = self
            .client_order_id
            .as_ref()
            .filter(|id| !id.is_empty())
            .map(|id| ClientOrderId::new(id))
            .or_else(|| {
                // If no client order ID, use order ID as fallback
                self.order_id
                    .as_ref()
                    .filter(|id| !id.is_empty())
                    .map(|id| ClientOrderId::new(id))
            });

        let venue_order_id = self
            .order_id
            .as_ref()
            .map(|id| VenueOrderId::new(id.clone()))
            .ok_or_else(|| anyhow::anyhow!("Order ID is missing"))?;

        let order_side = match self.side.as_str() {
            "BUY" => OrderSide::Buy,
            "SELL" => OrderSide::Sell,
            _ => anyhow::bail!("Invalid order side: {}", self.side),
        };

        let order_type = match self.order_type.as_deref() {
            Some("LIMIT") => OrderType::Limit,
            Some("MARKET") => OrderType::Market,
            Some("LIMIT_MAKER") => OrderType::Limit,
            Some("IMMEDIATE_OR_CANCEL") | Some("IOC") => OrderType::Market,
            Some("FILL_OR_KILL") | Some("FOK") => OrderType::Market,
            _ => OrderType::Market, // Default to Market
        };

        let time_in_force = TimeInForce::Gtc; // MEXC defaults to GTC

        let order_status = match self.status.as_deref() {
            Some("NEW") => OrderStatus::Accepted,
            Some("PARTIALLY_FILLED") => OrderStatus::PartiallyFilled,
            Some("FILLED") => OrderStatus::Filled,
            Some("CANCELED") | Some("PARTIALLY_CANCELED") => OrderStatus::Canceled,
            Some("REJECTED") => OrderStatus::Rejected,
            Some("EXPIRED") => OrderStatus::Expired,
            _ => OrderStatus::Accepted, // Default to Accepted
        };

        let quantity = Quantity::from_str(&self.quantity)
            .map_err(|e| anyhow::anyhow!("Failed to parse quantity '{}': {}", self.quantity, e))?;

        let filled_quantity = self
            .executed_quantity
            .as_ref()
            .map(|q| {
                Quantity::from_str(q).map_err(|e| {
                    anyhow::anyhow!("Failed to parse executed quantity '{}': {}", q, e)
                })
            })
            .transpose()?
            .unwrap_or_else(|| Quantity::zero(size_precision));

        // Parse price if available (required for Limit orders)
        let price = self
            .price
            .as_ref()
            .filter(|p| !p.is_empty())
            .and_then(|p| p.parse::<f64>().ok())
            .map(|px| Price::new(px, price_precision));

        // Build report with price if available
        let mut report = OrderStatusReport::new(
            account_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            order_side,
            order_type,
            time_in_force,
            order_status,
            quantity,
            filled_quantity,
            ts_event,
            ts_event,
            ts_now,
            Some(Uuid::new_v4().into()),
        );

        // Set price if available (required for Limit orders during reconciliation)
        if let Some(p) = price {
            report = report.with_price(p);
        }

        // Note: avg_px is not available from MEXC order query API
        // It can be calculated from filled quantity and amount if needed in the future

        Ok(report)
    }
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

/// MEXC listen key response for user data stream.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenKeyResponse {
    /// The listen key for WebSocket user data stream.
    pub listen_key: String,
}

