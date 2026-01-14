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

//! Error structures and enumerations for the MEXC integration.

use nautilus_network::http::{HttpClientError, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Build error for query parameter validation.
#[derive(Debug, Clone, Error)]
pub enum MexcBuildError {
    /// Missing required symbol.
    #[error("Missing required symbol")]
    MissingSymbol,
    /// Invalid limit value.
    #[error("Invalid limit: must be between 1 and 1000")]
    InvalidLimit,
    /// Invalid offset value.
    #[error("Invalid offset: must be non-negative")]
    InvalidOffset,
    /// Invalid time range: `start_time` should be less than `end_time`.
    #[error(
        "Invalid time range: start_time ({start_time}) must be less than end_time ({end_time})"
    )]
    InvalidTimeRange { start_time: i64, end_time: i64 },
    /// Both orderId and clientOrderId specified.
    #[error("Cannot specify both 'orderId' and 'clientOrderId'")]
    BothOrderIds,
    /// Missing required order identifier.
    #[error("Missing required order identifier (orderId or clientOrderId)")]
    MissingOrderId,
    /// Validation error.
    #[error("Validation error: {0}")]
    ValidationError(String),
}

/// Represents the JSON structure of an error response returned by the MEXC API.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MexcErrorResponse {
    /// The error code returned by MEXC.
    #[serde(rename = "code")]
    pub code: Option<i32>,
    /// The error message provided by MEXC.
    #[serde(rename = "msg")]
    pub message: Option<String>,
}

/// A typed error enumeration for the MEXC HTTP client.
#[derive(Debug, Clone, Error)]
pub enum MexcHttpError {
    /// Error variant when credentials are missing but the request is authenticated.
    #[error("Missing credentials for authenticated request")]
    MissingCredentials,
    /// Errors returned directly by MEXC.
    #[error("MEXC error (code: {code}): {message}")]
    MexcError { code: i32, message: String },
    /// Failure during JSON serialization/deserialization.
    #[error("JSON error: {0}")]
    JsonError(String),
    /// Parameter validation error.
    #[error("Parameter validation error: {0}")]
    ValidationError(String),
    /// Build error for query parameters.
    #[error("Build error: {0}")]
    BuildError(#[from] MexcBuildError),
    /// Request was canceled, typically due to shutdown or disconnect.
    #[error("Request canceled: {0}")]
    Canceled(String),
    /// Generic network error (for retries, cancellations, etc).
    #[error("Network error: {0}")]
    NetworkError(String),
    /// Any unknown HTTP status or unexpected response from MEXC.
    #[error("Unexpected HTTP status code {status}: {body}")]
    UnexpectedStatus { status: StatusCode, body: String },
}

impl From<HttpClientError> for MexcHttpError {
    fn from(error: HttpClientError) -> Self {
        Self::NetworkError(error.to_string())
    }
}

impl From<String> for MexcHttpError {
    fn from(error: String) -> Self {
        Self::ValidationError(error)
    }
}

// Allow use of the `?` operator on `serde_json` results inside the HTTP
// client implementation by converting them into our typed error.
impl From<serde_json::Error> for MexcHttpError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

impl From<MexcErrorResponse> for MexcHttpError {
    fn from(error: MexcErrorResponse) -> Self {
        let code = error.code.unwrap_or(-1);
        let message = error.message.unwrap_or_else(|| "Unknown error".to_string());
        Self::MexcError { code, message }
    }
}
