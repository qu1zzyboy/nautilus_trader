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

//! Provides the HTTP client integration for the [MEXC](https://www.mexc.com) REST API.
//!
//! This module defines and implements a [`MexcRawHttpClient`] for
//! sending requests to various MEXC endpoints. It handles request signing
//! (when credentials are provided), constructs valid HTTP requests
//! using the [`HttpClient`], and parses the responses back into structured data or a [`MexcHttpError`].
//!
//! MEXC API reference <https://mexcdevelop.github.io/apidocs/spot_v3_en/>.

use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::LazyLock,
};

use chrono::Utc;
use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_network::{
    http::{HttpClient, Method, StatusCode, USER_AGENT},
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use serde::{Serialize, de::DeserializeOwned};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    error::{MexcErrorResponse, MexcHttpError},
    models::{
        ListenKeyResponse, MexcAccount, MexcInstrument, MexcKline, MexcOrder, MexcOrderBook,
        MexcTicker, MexcTrade,
    },
    query::{
        DeleteOrderParams, GetAccountParams, GetDepthParams, GetExchangeInfoParams,
        GetKlinesParams, GetOpenOrdersParams, GetOrderParams, GetTicker24hrParams, GetTradesParams,
        ListenKeyParams, PostOrderParams,
    },
};
use crate::{
    common::{
        consts::MEXC_HTTP_URL,
        credential::Credential,
    },
};

/// Default MEXC REST API rate limits.
///
/// MEXC implements rate limiting:
/// - 1200 requests per minute for authenticated users.
/// - 10 requests per second burst limit.
const MEXC_DEFAULT_RATE_LIMIT_PER_SECOND: u32 = 10;
const MEXC_DEFAULT_RATE_LIMIT_PER_MINUTE_AUTHENTICATED: u32 = 1200;
const MEXC_DEFAULT_RATE_LIMIT_PER_MINUTE_UNAUTHENTICATED: u32 = 1200;

const MEXC_GLOBAL_RATE_KEY: &str = "mexc:global";
const MEXC_MINUTE_RATE_KEY: &str = "mexc:minute";

static RATE_LIMIT_KEYS: LazyLock<Vec<Ustr>> = LazyLock::new(|| {
    vec![
        Ustr::from(MEXC_GLOBAL_RATE_KEY),
        Ustr::from(MEXC_MINUTE_RATE_KEY),
    ]
});

/// Provides a lower-level HTTP client for connecting to the [MEXC](https://www.mexc.com) REST API.
///
/// This client wraps the underlying [`HttpClient`] to handle functionality
/// specific to MEXC, such as request signing (for authenticated endpoints),
/// forming request URLs, and deserializing responses into specific data models.
///
/// # Connection Management
///
/// The client uses HTTP keep-alive for connection pooling with a 90-second idle timeout.
/// Connections are automatically reused for subsequent requests to minimize latency.
///
/// # Rate Limiting
///
/// MEXC enforces the following rate limits:
/// - 1200 requests per minute for authenticated and unauthenticated users.
/// - 10 requests per second burst limit for certain endpoints.
///
/// The client automatically respects these limits through the configured quota.
#[derive(Debug, Clone)]
pub struct MexcRawHttpClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
    retry_manager: RetryManager<MexcHttpError>,
    cancellation_token: CancellationToken,
}

impl Default for MexcRawHttpClient {
    fn default() -> Self {
        Self::new(
            None,    // base_url
            Some(60), // timeout_secs
            None,    // max_retries
            None,    // retry_delay_ms
            None,    // retry_delay_max_ms
            None,    // max_requests_per_second
            None,    // max_requests_per_minute
            None,    // proxy_url
        )
        .expect("Failed to create default MexcRawHttpClient")
    }
}

impl MexcRawHttpClient {
    /// Creates a new [`MexcRawHttpClient`] using the default MEXC HTTP URL,
    /// optionally overridden with a custom base URL.
    ///
    /// This version of the client has **no credentials**, so it can only
    /// call publicly accessible endpoints.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        max_requests_per_second: Option<u32>,
        max_requests_per_minute: Option<u32>,
        proxy_url: Option<String>,
    ) -> Result<Self, MexcHttpError> {
        let retry_config = RetryConfig {
            max_retries: max_retries.unwrap_or(3),
            initial_delay_ms: retry_delay_ms.unwrap_or(1000),
            max_delay_ms: retry_delay_max_ms.unwrap_or(10_000),
            backoff_factor: 2.0,
            jitter_ms: 1000,
            operation_timeout_ms: Some(60_000),
            immediate_first: false,
            max_elapsed_ms: Some(180_000),
        };

        let retry_manager = RetryManager::new(retry_config);

        let max_req_per_sec =
            max_requests_per_second.unwrap_or(MEXC_DEFAULT_RATE_LIMIT_PER_SECOND);
        let max_req_per_min =
            max_requests_per_minute.unwrap_or(MEXC_DEFAULT_RATE_LIMIT_PER_MINUTE_UNAUTHENTICATED);

        Ok(Self {
            base_url: base_url.unwrap_or(MEXC_HTTP_URL.to_string()),
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(max_req_per_sec, max_req_per_min),
                Some(Self::default_quota(max_req_per_sec)),
                timeout_secs,
                proxy_url,
            )
            .map_err(|e| {
                MexcHttpError::NetworkError(format!("Failed to create HTTP client: {e}"))
            })?,
            credential: None,
            retry_manager,
            cancellation_token: CancellationToken::new(),
        })
    }

    /// Creates a new [`MexcRawHttpClient`] configured with credentials
    /// for authenticated requests, optionally using a custom base URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: String,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        max_requests_per_second: Option<u32>,
        max_requests_per_minute: Option<u32>,
        proxy_url: Option<String>,
    ) -> Result<Self, MexcHttpError> {
        let retry_config = RetryConfig {
            max_retries: max_retries.unwrap_or(3),
            initial_delay_ms: retry_delay_ms.unwrap_or(1000),
            max_delay_ms: retry_delay_max_ms.unwrap_or(10_000),
            backoff_factor: 2.0,
            jitter_ms: 1000,
            operation_timeout_ms: Some(60_000),
            immediate_first: false,
            max_elapsed_ms: Some(180_000),
        };

        let retry_manager = RetryManager::new(retry_config);

        let max_req_per_sec =
            max_requests_per_second.unwrap_or(MEXC_DEFAULT_RATE_LIMIT_PER_SECOND);
        let max_req_per_min =
            max_requests_per_minute.unwrap_or(MEXC_DEFAULT_RATE_LIMIT_PER_MINUTE_AUTHENTICATED);

        Ok(Self {
            base_url,
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(max_req_per_sec, max_req_per_min),
                Some(Self::default_quota(max_req_per_sec)),
                timeout_secs,
                proxy_url,
            )
            .map_err(|e| {
                MexcHttpError::NetworkError(format!("Failed to create HTTP client: {e}"))
            })?,
            credential: Some(Credential::new(api_key, api_secret)),
            retry_manager,
            cancellation_token: CancellationToken::new(),
        })
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())])
    }

    fn default_quota(max_requests_per_second: u32) -> Quota {
        Quota::per_second(
            NonZeroU32::new(max_requests_per_second)
                .unwrap_or_else(|| NonZeroU32::new(MEXC_DEFAULT_RATE_LIMIT_PER_SECOND).unwrap()),
        )
    }

    fn rate_limiter_quotas(
        max_requests_per_second: u32,
        max_requests_per_minute: u32,
    ) -> Vec<(String, Quota)> {
        let per_sec_quota = Quota::per_second(
            NonZeroU32::new(max_requests_per_second)
                .unwrap_or_else(|| NonZeroU32::new(MEXC_DEFAULT_RATE_LIMIT_PER_SECOND).unwrap()),
        );
        let per_min_quota =
            Quota::per_minute(NonZeroU32::new(max_requests_per_minute).unwrap_or_else(|| {
                NonZeroU32::new(MEXC_DEFAULT_RATE_LIMIT_PER_MINUTE_AUTHENTICATED).unwrap()
            }));

        vec![
            (MEXC_GLOBAL_RATE_KEY.to_string(), per_sec_quota),
            (MEXC_MINUTE_RATE_KEY.to_string(), per_min_quota),
        ]
    }

    fn rate_limit_keys() -> Vec<Ustr> {
        RATE_LIMIT_KEYS.clone()
    }

    /// Cancel all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Signs a request according to MEXC authentication scheme.
    ///
    /// MEXC uses HMAC SHA256 to sign query parameters.
    /// The signature is computed from sorted query parameters.
    fn sign_request(
        &self,
        query_string: &str,
    ) -> Result<String, MexcHttpError> {
        let credential = self
            .credential
            .as_ref()
            .ok_or(MexcHttpError::MissingCredentials)?;

        Ok(credential.sign(query_string))
    }

    async fn send_request<T: DeserializeOwned, P: Serialize>(
        &self,
        method: Method,
        endpoint: &str,
        params: Option<&P>,
        body: Option<Vec<u8>>,
        authenticate: bool,
    ) -> Result<T, MexcHttpError> {
        let endpoint = endpoint.to_string();
        let method_clone = method.clone();
        let body_clone = body.clone();

        // Serialize params before closure to avoid reference lifetime issues
        // Query params are used with GET, DELETE, and PUT methods
        let params_str = if method == Method::GET || method == Method::DELETE || method == Method::PUT {
            params
                .map(serde_urlencoded::to_string)
                .transpose()
                .map_err(|e| {
                    MexcHttpError::JsonError(format!("Failed to serialize params: {e}"))
                })?
        } else {
            None
        };

        // MEXC uses query string for all parameters, even for POST requests
        // Convert body parameters to query string if present
        let mut query_params = params_str.as_deref().unwrap_or("").to_string();
        
        // If POST request with body, convert body parameters to query string
        if method == Method::POST {
            if let Some(ref body_bytes) = body {
                // MEXC uses form-encoded body (key1=value1&key2=value2)
                if let Ok(body_str) = std::str::from_utf8(body_bytes) {
                    if !body_str.is_empty() {
                        if !query_params.is_empty() {
                            query_params.push_str(&format!("&{}", body_str));
                        } else {
                            query_params = body_str.to_string();
                        }
                    }
                }
            }
        }
        
        // Build initial endpoint with query params
        let mut full_endpoint = if !query_params.is_empty() {
            format!("{endpoint}?{query_params}")
        } else {
            endpoint.clone()
        };

        // Initialize final_body as None (MEXC doesn't use body for authenticated requests)
        let final_body: Option<Vec<u8>> = None;

        if authenticate {
            // MEXC requires timestamp parameter for authenticated requests
            let timestamp = Utc::now().timestamp_millis();
            
            // Build all parameters including timestamp (but not signature)
            // Convert to Vec<String> of "key=value" format for sorting (like reference code)
            let mut param_pairs: Vec<String> = if !query_params.is_empty() {
                query_params.split('&').map(|s| s.to_string()).collect()
            } else {
                Vec::new()
            };
            
            // Add timestamp to parameters
            param_pairs.push(format!("timestamp={timestamp}"));
            
            // Sort all parameters alphabetically (MEXC requirement)
            // This sorts the entire "key=value" strings, which is equivalent to sorting by key
            param_pairs.sort();
            
            // Join sorted parameters
            let sorted_query = param_pairs.join("&");
            
            // Debug: log the query string used for signing
            log::info!("MEXC signature query string: {}", sorted_query);
            
            let signature = self.sign_request(&sorted_query)?;
            
            log::info!("MEXC signature: {}", signature);
            
            // Add signature to query string (timestamp is already in sorted_query)
            // Build final query string: sorted_query + signature (like reference code)
            let final_query = format!("{}&signature={}", sorted_query, signature);
            
            // Update endpoint with final query string
            // Remove existing query params and replace with final_query
            if let Some((base, _)) = full_endpoint.split_once('?') {
                full_endpoint = format!("{}?{}", base, final_query);
            } else {
                full_endpoint = format!("{}?{}", full_endpoint, final_query);
            }
        }

        let url = format!("{}{}", self.base_url, full_endpoint);

        let operation = || {
            let url = url.clone();
            let method = method_clone.clone();
            let body = final_body.clone();

            async move {
                let mut headers = Self::default_headers();
                
                // Add API key header for authenticated requests
                if authenticate {
                    if let Some(credential) = &self.credential {
                        headers.insert("X-MEXC-APIKEY".to_string(), credential.api_key.to_string());
                    }
                }
                
                // MEXC doesn't require Content-Type header for POST requests
                // All parameters are in query string, not body

                let rate_keys = Self::rate_limit_keys();
                let resp = self
                    .client
                    .request_with_ustr_keys(method, url, None, Some(headers), body, None, Some(rate_keys))
                    .await?;

                if resp.status.is_success() {
                    serde_json::from_slice(&resp.body).map_err(Into::into)
                } else if let Ok(error_resp) =
                    serde_json::from_slice::<MexcErrorResponse>(&resp.body)
                {
                    Err(error_resp.into())
                } else {
                    Err(MexcHttpError::UnexpectedStatus {
                        status: StatusCode::from_u16(resp.status.as_u16())
                            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        body: String::from_utf8_lossy(&resp.body).to_string(),
                    })
                }
            }
        };

        // Retry strategy for MEXC
        let should_retry = |error: &MexcHttpError| -> bool {
            match error {
                MexcHttpError::NetworkError(_) => true,
                MexcHttpError::UnexpectedStatus { status, .. } => {
                    status.as_u16() >= 500 || status.as_u16() == 429
                }
                MexcHttpError::MexcError { code, .. } => {
                    // Retry on rate limit errors
                    *code == 429
                }
                _ => false,
            }
        };

        let create_error = |msg: String| -> MexcHttpError {
            if msg == "canceled" {
                MexcHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
            } else {
                MexcHttpError::NetworkError(msg)
            }
        };

        self.retry_manager
            .execute_with_retry_with_cancel(
                endpoint.as_str(),
                operation,
                should_retry,
                create_error,
                &self.cancellation_token,
            )
            .await
    }

    /// Get exchange information (instruments).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_exchange_info(
        &self,
        params: Option<GetExchangeInfoParams>,
    ) -> Result<Vec<MexcInstrument>, MexcHttpError> {
        self.send_request::<_, _>(Method::GET, "/api/v3/exchangeInfo", params.as_ref(), None, false)
            .await
    }

    /// Get recent trades.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_trades(
        &self,
        params: GetTradesParams,
    ) -> Result<Vec<MexcTrade>, MexcHttpError> {
        self.send_request(Method::GET, "/api/v3/trades", Some(&params), None, false)
            .await
    }

    /// Get order book depth.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_depth(
        &self,
        params: GetDepthParams,
    ) -> Result<MexcOrderBook, MexcHttpError> {
        self.send_request(Method::GET, "/api/v3/depth", Some(&params), None, false)
            .await
    }

    /// Get kline/candlestick data.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_klines(
        &self,
        params: GetKlinesParams,
    ) -> Result<Vec<MexcKline>, MexcHttpError> {
        self.send_request(Method::GET, "/api/v3/klines", Some(&params), None, false)
            .await
    }

    /// Get 24hr ticker statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_ticker_24hr(
        &self,
        params: Option<GetTicker24hrParams>,
    ) -> Result<Vec<MexcTicker>, MexcHttpError> {
        self.send_request::<_, _>(Method::GET, "/api/v3/ticker/24hr", params.as_ref(), None, false)
            .await
    }

    /// Get account information.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    pub async fn get_account(
        &self,
        _params: GetAccountParams,
    ) -> Result<MexcAccount, MexcHttpError> {
        self.send_request::<_, ()>(Method::GET, "/api/v3/account", None, None, true)
            .await
    }

    /// Place a new order.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    pub async fn place_order(&self, params: PostOrderParams) -> Result<MexcOrder, MexcHttpError> {
        // MEXC uses query string for POST requests, not body
        // Convert params to form-encoded format for query string
        // Build all parameters first, then sort alphabetically (MEXC requirement)
        let mut form_params: Vec<(String, String)> = vec![
            ("symbol".to_string(), params.symbol),
            ("side".to_string(), params.side),
            ("type".to_string(), params.order_type),
        ];
        
        if let Some(quantity) = params.quantity {
            form_params.push(("quantity".to_string(), quantity));
        }
        if let Some(price) = params.price {
            form_params.push(("price".to_string(), price));
        }
        if let Some(new_client_order_id) = params.new_client_order_id {
            form_params.push(("newClientOrderId".to_string(), new_client_order_id));
        }
        // Note: MEXC API doesn't support timeInForce parameter
        if let Some(stop_price) = params.stop_price {
            form_params.push(("stopPrice".to_string(), stop_price));
        }
        
        // Sort parameters alphabetically by key (MEXC requirement)
        form_params.sort_by(|a, b| a.0.cmp(&b.0));
        
        // Convert to form-encoded string for body (will be moved to query string in send_request)
        let body_str = form_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");
        
        let body = body_str.as_bytes().to_vec();
        
        // send_request will move body params to query string for POST requests
        // and add timestamp, then sort again before signing
        self.send_request::<_, ()>(Method::POST, "/api/v3/order", None, Some(body), true)
            .await
    }

    /// Get order information.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    pub async fn get_order(
        &self,
        params: GetOrderParams,
    ) -> Result<MexcOrder, MexcHttpError> {
        self.send_request(Method::GET, "/api/v3/order", Some(&params), None, true)
            .await
    }

    /// Cancel an order.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    pub async fn cancel_order(
        &self,
        params: DeleteOrderParams,
    ) -> Result<MexcOrder, MexcHttpError> {
        self.send_request(Method::DELETE, "/api/v3/order", Some(&params), None, true)
            .await
    }

    /// Get open orders.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    pub async fn get_open_orders(
        &self,
        params: Option<GetOpenOrdersParams>,
    ) -> Result<Vec<MexcOrder>, MexcHttpError> {
        self.send_request::<_, _>(Method::GET, "/api/v3/openOrders", params.as_ref(), None, true)
            .await
    }

    /// Creates a listen key for user data stream.
    ///
    /// Listen keys are valid for 60 minutes. Use `keepalive_listen_key` to keep
    /// the stream alive.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the request fails.
    pub async fn create_listen_key(&self) -> Result<ListenKeyResponse, MexcHttpError> {
        // MEXC uses POST with API key authentication (no signature required for user data stream)
        self.send_request(Method::POST, "/api/v3/userDataStream", None::<&()>, None, true)
            .await
    }

    /// Keeps alive an existing listen key.
    ///
    /// Should be called periodically to keep the user data stream alive.
    /// Extends the validity of the listen key by 60 minutes.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the request fails.
    pub async fn keepalive_listen_key(&self, listen_key: &str) -> Result<(), MexcHttpError> {
        let params = ListenKeyParams {
            listen_key: listen_key.to_string(),
        };
        // MEXC uses PUT with query parameters and API key authentication
        // send_request now handles PUT query params correctly
        let _: serde_json::Value = self
            .send_request(Method::PUT, "/api/v3/userDataStream", Some(&params), None, true)
            .await?;
        Ok(())
    }

    /// Closes an existing listen key.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the request fails.
    pub async fn close_listen_key(&self, listen_key: &str) -> Result<(), MexcHttpError> {
        let params = ListenKeyParams {
            listen_key: listen_key.to_string(),
        };
        // MEXC uses DELETE with query parameters and API key authentication
        // DELETE requests use query params, so send_request handles it correctly
        let _: serde_json::Value = self
            .send_request(Method::DELETE, "/api/v3/userDataStream", Some(&params), None, true)
            .await?;
        Ok(())
    }
}

/// High-level HTTP client wrapper (for future use).
#[derive(Debug, Clone)]
pub struct MexcHttpClient {
    inner: MexcRawHttpClient,
}

impl MexcHttpClient {
    /// Creates a new [`MexcHttpClient`] instance.
    pub fn new() -> Self {
        Self {
            inner: MexcRawHttpClient::default(),
        }
    }
}

impl Default for MexcHttpClient {
    fn default() -> Self {
        Self::new()
    }
}
