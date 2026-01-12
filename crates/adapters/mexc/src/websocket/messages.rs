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

//! WebSocket message types for MEXC adapter.

use nautilus_model::data::Data;
use serde::{Deserialize, Deserializer, Serializer, de};
#[derive(Debug, Clone, Serialize)]
/// Internal WebSocket message type for MEXC.
pub struct MEXCAuthentication {}
#[derive(Clone, Debug)]
pub enum MexcWsMessage {
    /// Reconnection signal
    Reconnected,
    /// Subscription confirmation or error.
    Subscription {
        success: bool,
        topic: Option<String>,
        error: Option<String>,
    },
    /// Market data message.
    Data(Vec<Data>),
}

/// Nautilus WebSocket message wrapper.
#[derive(Clone, Debug)]
pub enum NautilusWsMessage {
    /// Reconnection signal.
    Reconnected,
    /// Market data.
    Data(Vec<Data>),
}
