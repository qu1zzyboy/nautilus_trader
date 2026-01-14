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

//! Unified error handling for the MEXC adapter.

use thiserror::Error;

/// The main error type for all MEXC adapter operations.
#[derive(Debug, Error)]
pub enum MexcError {
    /// Invalid order side.
    #[error("Invalid order side: {0}")]
    InvalidOrderSide(String),
    /// Invalid order type.
    #[error("Invalid order type: {0}")]
    InvalidOrderType(String),
    /// Invalid order status.
    #[error("Invalid order status: {0}")]
    InvalidOrderStatus(String),
    /// Invalid time in force.
    #[error("Invalid time in force: {0}")]
    InvalidTimeInForce(String),
    /// Validation error.
    #[error("Validation error: {0}")]
    Validation(String),
    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),
}

/// WebSocket-specific errors for MEXC adapter.
#[derive(Debug, Error)]
pub enum MexcWsError {
    /// Client connection or communication error.
    #[error("Client error: {0}")]
    ClientError(String),

    /// Authentication error.
    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    /// Subscription error.
    #[error("Subscription error: {0}")]
    SubscriptionError(String),

    /// Missing credentials for authenticated operation.
    #[error("Missing credentials")]
    MissingCredentials,

    /// Protobuf encoding error.
    #[error("Protobuf encoding error: {0}")]
    EncodingError(String),

    /// Protobuf decoding error.
    #[error("Protobuf decoding error: {0}")]
    DecodingError(String),

    /// Invalid message format.
    #[error("Invalid message format: {0}")]
    InvalidMessage(String),
}

/// HTTP-specific errors for MEXC adapter.
#[derive(Debug, Error)]
pub enum MexcHttpError {
    /// HTTP request error.
    #[error("HTTP request error: {0}")]
    RequestError(String),

    /// HTTP response error.
    #[error("HTTP response error: {0}")]
    ResponseError(String),

    /// Invalid API credentials.
    #[error("Invalid API credentials")]
    InvalidCredentials,
}

