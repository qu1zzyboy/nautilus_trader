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

//! WebSocket-specific errors for MEXC adapter.

use thiserror::Error;

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

    /// Parse error when converting protobuf to Nautilus types.
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Missing required field in protobuf message.
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Invalid instrument symbol.
    #[error("Invalid instrument symbol: {0}")]
    InvalidSymbol(String),
}

/// Result type for MEXC WebSocket operations.
pub type MexcWsResult<T> = Result<T, MexcWsError>;

