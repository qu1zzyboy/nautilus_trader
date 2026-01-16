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

//! Builder types for MEXC REST query parameters.

use derive_builder::Builder;
use serde::{Deserialize, Serialize};

/// Parameters for the GET /api/v3/exchangeInfo endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetExchangeInfoParams {
    /// Trading symbol (optional, if not provided returns all symbols).
    pub symbol: Option<String>,
}

/// Parameters for the GET /api/v3/trades endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetTradesParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Number of trades to return (default: 100, max: 1000).
    pub limit: Option<u32>,
}

/// Parameters for the GET /api/v3/depth endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetDepthParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Order book depth (default: 100, max: 5000).
    pub limit: Option<u32>,
}

/// Parameters for the GET /api/v3/klines endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetKlinesParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Kline interval (e.g., "1m", "5m", "1h", "1d").
    pub interval: String,
    /// Start time (optional).
    pub start_time: Option<i64>,
    /// End time (optional).
    pub end_time: Option<i64>,
    /// Number of klines to return (default: 500, max: 1000).
    pub limit: Option<u32>,
}

/// Parameters for the GET /api/v3/ticker/24hr endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetTicker24hrParams {
    /// Trading symbol (optional, if not provided returns all tickers).
    pub symbol: Option<String>,
}

/// Parameters for the POST /api/v3/order endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct PostOrderParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Order side: BUY or SELL (required).
    pub side: String,
    /// Order type: LIMIT or MARKET (required).
    #[serde(rename = "type")]
    pub order_type: String,
    /// Order quantity (required).
    pub quantity: Option<String>,
    /// Order price (required for LIMIT orders).
    pub price: Option<String>,
    /// Client order ID (optional).
    pub new_client_order_id: Option<String>,
    /// Time in force: GTC, IOC, FOK (optional, default: GTC).
    pub time_in_force: Option<String>,
    /// Stop price (optional, for STOP orders).
    pub stop_price: Option<String>,
}

/// Parameters for the GET /api/v3/order endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetOrderParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Order ID (optional, mutually exclusive with orig_client_order_id).
    pub order_id: Option<String>,
    /// Client order ID (optional, mutually exclusive with order_id).
    pub orig_client_order_id: Option<String>,
}

/// Parameters for the DELETE /api/v3/order endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct DeleteOrderParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Order ID (optional, mutually exclusive with orig_client_order_id).
    pub order_id: Option<String>,
    /// Client order ID (optional, mutually exclusive with order_id).
    pub orig_client_order_id: Option<String>,
}

/// Parameters for the GET /api/v3/openOrders endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetOpenOrdersParams {
    /// Trading symbol (optional, if not provided returns all open orders).
    pub symbol: Option<String>,
}

/// Parameters for the GET /api/v3/account endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetAccountParams {
    // No parameters required for account info
}

/// Parameters for PUT/DELETE /api/v3/userDataStream endpoint.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenKeyParams {
    /// The listen key to extend or close.
    pub listen_key: String,
}

impl GetTradesParamsBuilder {
    /// Validates and builds the parameters.
    pub fn build_validated(&self) -> Result<GetTradesParams, crate::http::error::MexcBuildError> {
        let params = self.build().map_err(|_| {
            crate::http::error::MexcBuildError::ValidationError("Failed to build params".to_string())
        })?;

        if params.symbol.is_empty() {
            return Err(crate::http::error::MexcBuildError::MissingSymbol);
        }

        if let Some(limit) = params.limit {
            if limit == 0 || limit > 1000 {
                return Err(crate::http::error::MexcBuildError::InvalidLimit);
            }
        }

        Ok(params)
    }
}

impl GetDepthParamsBuilder {
    /// Validates and builds the parameters.
    pub fn build_validated(&self) -> Result<GetDepthParams, crate::http::error::MexcBuildError> {
        let params = self.build().map_err(|_| {
            crate::http::error::MexcBuildError::ValidationError("Failed to build params".to_string())
        })?;

        if params.symbol.is_empty() {
            return Err(crate::http::error::MexcBuildError::MissingSymbol);
        }

        if let Some(limit) = params.limit {
            if limit == 0 || limit > 5000 {
                return Err(crate::http::error::MexcBuildError::InvalidLimit);
            }
        }

        Ok(params)
    }
}

impl GetKlinesParamsBuilder {
    /// Validates and builds the parameters.
    pub fn build_validated(&self) -> Result<GetKlinesParams, crate::http::error::MexcBuildError> {
        let params = self.build().map_err(|_| {
            crate::http::error::MexcBuildError::ValidationError("Failed to build params".to_string())
        })?;

        if params.symbol.is_empty() {
            return Err(crate::http::error::MexcBuildError::MissingSymbol);
        }

        if let Some(start) = params.start_time {
            if let Some(end) = params.end_time {
                if start >= end {
                    return Err(crate::http::error::MexcBuildError::InvalidTimeRange {
                        start_time: start,
                        end_time: end,
                    });
                }
            }
        }

        if let Some(limit) = params.limit {
            if limit == 0 || limit > 1000 {
                return Err(crate::http::error::MexcBuildError::InvalidLimit);
            }
        }

        Ok(params)
    }
}

impl PostOrderParamsBuilder {
    /// Validates and builds the parameters.
    pub fn build_validated(&self) -> Result<PostOrderParams, crate::http::error::MexcBuildError> {
        let params = self.build().map_err(|_| {
            crate::http::error::MexcBuildError::ValidationError("Failed to build params".to_string())
        })?;

        if params.symbol.is_empty() {
            return Err(crate::http::error::MexcBuildError::MissingSymbol);
        }

        Ok(params)
    }
}

impl GetOrderParamsBuilder {
    /// Validates and builds the parameters.
    pub fn build_validated(&self) -> Result<GetOrderParams, crate::http::error::MexcBuildError> {
        let params = self.build().map_err(|_| {
            crate::http::error::MexcBuildError::ValidationError("Failed to build params".to_string())
        })?;

        if params.symbol.is_empty() {
            return Err(crate::http::error::MexcBuildError::MissingSymbol);
        }

        if params.order_id.is_some() && params.orig_client_order_id.is_some() {
            return Err(crate::http::error::MexcBuildError::BothOrderIds);
        }

        if params.order_id.is_none() && params.orig_client_order_id.is_none() {
            return Err(crate::http::error::MexcBuildError::MissingOrderId);
        }

        Ok(params)
    }
}

impl DeleteOrderParamsBuilder {
    /// Validates and builds the parameters.
    pub fn build_validated(&self) -> Result<DeleteOrderParams, crate::http::error::MexcBuildError> {
        let params = self.build().map_err(|_| {
            crate::http::error::MexcBuildError::ValidationError("Failed to build params".to_string())
        })?;

        if params.symbol.is_empty() {
            return Err(crate::http::error::MexcBuildError::MissingSymbol);
        }

        if params.order_id.is_some() && params.orig_client_order_id.is_some() {
            return Err(crate::http::error::MexcBuildError::BothOrderIds);
        }

        if params.order_id.is_none() && params.orig_client_order_id.is_none() {
            return Err(crate::http::error::MexcBuildError::MissingOrderId);
        }

        Ok(params)
    }
}

