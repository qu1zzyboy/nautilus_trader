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

//! Per-bar net taker imbalance ratio in `[-1, 1]`.
//!
//! For a Binance kline with quote volume `Q` and taker buy quote volume `B`:
//!
//! ```text
//! sell_quote_volume   = Q - B
//! net_taker_imbalance = (B - sell_quote_volume) / max(Q, eps)
//!                     = (2 * B - Q)             / max(Q, eps)
//! ```
//!
//! Stateless (no rolling window); only depends on the current bar.

use std::fmt::Display;

use nautilus_model::data::BnBar;

use crate::indicator::Indicator;

/// Denominator clip used to match the notebook.
pub const DEFAULT_EPSILON: f64 = 1e-12;

/// Per-bar net taker imbalance ratio (Binance-only).
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.indicators")
)]
pub struct NetTakerImbalance {
    pub epsilon: f64,
    pub value: f64,
    pub initialized: bool,
    has_inputs: bool,
}

impl Display for NetTakerImbalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl Indicator for NetTakerImbalance {
    fn name(&self) -> String {
        stringify!(NetTakerImbalance).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bn_bar(&mut self, bar: &BnBar) {
        self.update_raw(
            (&bar.quote_volume).into(),
            (&bar.taker_buy_quote_volume).into(),
        );
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.initialized = false;
        self.has_inputs = false;
    }
}

impl Default for NetTakerImbalance {
    fn default() -> Self {
        Self::new(None)
    }
}

impl NetTakerImbalance {
    /// Creates a new [`NetTakerImbalance`].
    ///
    /// `epsilon` defaults to [`DEFAULT_EPSILON`].
    #[must_use]
    pub fn new(epsilon: Option<f64>) -> Self {
        Self {
            epsilon: epsilon.unwrap_or(DEFAULT_EPSILON),
            value: 0.0,
            initialized: false,
            has_inputs: false,
        }
    }

    /// Updates with raw `quote_volume` and `taker_buy_quote_volume`.
    pub fn update_raw(&mut self, quote_volume: f64, taker_buy_quote_volume: f64) {
        let qv_clipped = quote_volume.max(self.epsilon);
        let sell_quote_volume = quote_volume - taker_buy_quote_volume;
        self.value = (taker_buy_quote_volume - sell_quote_volume) / qv_clipped;
        self.has_inputs = true;
        self.initialized = true;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::NetTakerImbalance;
    use crate::indicator::Indicator;

    #[rstest]
    fn test_display_and_default_not_initialized() {
        let ind = NetTakerImbalance::default();
        assert_eq!(format!("{ind}"), "NetTakerImbalance");
        assert!(!ind.initialized());
        assert!(!ind.has_inputs());
        assert_eq!(ind.value, 0.0);
    }

    #[rstest]
    fn test_balanced_returns_zero() {
        let mut ind = NetTakerImbalance::default();
        ind.update_raw(1_000.0, 500.0);
        assert!(ind.initialized());
        assert!(ind.has_inputs());
        assert!((ind.value - 0.0).abs() < 1e-12);
    }

    #[rstest]
    fn test_all_taker_buy_returns_one() {
        let mut ind = NetTakerImbalance::default();
        ind.update_raw(1_000.0, 1_000.0);
        assert!((ind.value - 1.0).abs() < 1e-12);
    }

    #[rstest]
    fn test_no_taker_buy_returns_neg_one() {
        let mut ind = NetTakerImbalance::default();
        ind.update_raw(1_000.0, 0.0);
        assert!((ind.value - -1.0).abs() < 1e-12);
    }

    #[rstest]
    fn test_zero_quote_volume_uses_epsilon() {
        let mut ind = NetTakerImbalance::new(Some(1e-9));
        // (0 - 0) / max(0, 1e-9) = 0
        ind.update_raw(0.0, 0.0);
        assert_eq!(ind.value, 0.0);
        assert!(ind.initialized());
    }

    #[rstest]
    fn test_reset() {
        let mut ind = NetTakerImbalance::default();
        ind.update_raw(1_000.0, 800.0);
        assert!(ind.initialized());
        ind.reset();
        assert!(!ind.initialized());
        assert!(!ind.has_inputs());
        assert_eq!(ind.value, 0.0);
    }
}
