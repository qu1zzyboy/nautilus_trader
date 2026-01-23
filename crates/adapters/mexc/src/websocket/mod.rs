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

//! WebSocket client implementation for MEXC real-time data feeds.
//!
//! This module provides a WebSocket client for subscribing to MEXC's real-time data streams.
//! MEXC uses Protocol Buffers (protobuf) for WebSocket communication, requiring binary
//! message handling.
//!
//! It supports:
//! - Public market data subscriptions (trades, quotes, order book updates, klines).
//! - Binary protobuf message encoding/decoding.
//! - Automatic reconnection and subscription management.
//! - Message parsing into Nautilus domain models.

pub mod client;
pub mod enums;
pub mod error;
pub mod handler;
pub mod handler_exec;
pub mod messages;
pub mod parse;

pub use crate::websocket::client::MexcWebSocketClient;
pub use crate::websocket::handler_exec::MexcExecWsFeedHandler;

