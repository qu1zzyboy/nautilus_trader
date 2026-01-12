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

//! Configuration types for the MEXC adapter clients.

use crate::common::consts::{MEXC_HTTP_URL, MEXC_WS_URL};

/// Configuration for the MEXC live data client.
#[derive(Clone, Debug)]
pub struct MexcDataClientConfig {
    /// Optional API key used for authenticated REST/WebSocket requests.
    pub api_key: Option<String>,
    /// Optional API secret used for authenticated REST/WebSocket requests.
    pub api_secret: Option<String>,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional HTTP proxy URL for general HTTP client operations.
    pub http_proxy_url: Option<String>,
    /// Optional REST timeout in seconds.
    pub http_timeout_secs: Option<u64>,
    /// Optional maximum retry attempts for REST requests.
    pub max_retries: Option<u32>,
    /// Optional heartbeat interval (seconds) for the WebSocket client.
    pub heartbeat_interval_secs: Option<u64>,
}

impl Default for MexcDataClientConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_secret: None,
            base_url_http: Some(MEXC_HTTP_URL.to_string()),
            base_url_ws: Some(MEXC_WS_URL.to_string()),
            http_proxy_url: None,
            http_timeout_secs: Some(30),
            max_retries: Some(3),
            heartbeat_interval_secs: Some(30),
        }
    }
}

impl MexcDataClientConfig {
    /// Returns the HTTP base URL.
    #[must_use]
    pub fn http_base_url(&self) -> &str {
        self.base_url_http.as_deref().unwrap_or(MEXC_HTTP_URL)
    }

    /// Returns the WebSocket URL.
    #[must_use]
    pub fn ws_url(&self) -> &str {
        self.base_url_ws.as_deref().unwrap_or(MEXC_WS_URL)
    }
}

