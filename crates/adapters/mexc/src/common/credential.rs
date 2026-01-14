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

//! API credential utilities for signing MEXC requests.

use std::fmt::Debug;

use aws_lc_rs::hmac;
use nautilus_core::string::mask_api_key;
use ustr::Ustr;
use zeroize::ZeroizeOnDrop;

/// MEXC API credentials for signing requests.
///
/// Uses HMAC SHA256 for request signing as per MEXC API specifications.
/// Secrets are automatically zeroized on drop for security.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    #[zeroize(skip)]
    pub api_key: Ustr,
    api_secret: Box<[u8]>,
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Credential))
            .field("api_key", &self.api_key)
            .field("api_secret", &"<redacted>")
            .finish()
    }
}

impl Credential {
    /// Creates a new [`Credential`] instance.
    #[must_use]
    pub fn new(api_key: String, api_secret: String) -> Self {
        let boxed: Box<[u8]> = api_secret.into_bytes().into_boxed_slice();

        Self {
            api_key: api_key.into(),
            api_secret: boxed,
        }
    }

    /// Signs a request message according to the MEXC authentication scheme.
    ///
    /// MEXC uses HMAC SHA256 to sign requests. The signature is computed from:
    /// - Query string parameters (sorted alphabetically)
    /// - Request body (if present)
    ///
    /// The signature is then added as a query parameter or header.
    #[must_use]
    pub fn sign(&self, query_string: &str) -> String {
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret[..]);
        let signature = hmac::sign(&key, query_string.as_bytes());
        hex::encode(signature.as_ref())
    }

    /// Returns a masked version of the API key for logging purposes.
    ///
    /// Shows first 4 and last 4 characters with ellipsis in between.
    /// For keys shorter than 8 characters, shows asterisks only.
    #[must_use]
    pub fn api_key_masked(&self) -> String {
        mask_api_key(self.api_key.as_str())
    }
}

