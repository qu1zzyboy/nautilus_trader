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

//! Raw net imbalance factor (no smoothing, no normalization).
//!
//! Per-bar formula (Binance kline):
//!
//! ```text
//! close_delta             = close - prev_close
//! ret_sign                = sign(close_delta)
//! qv_clipped              = max(quote_volume, eps)
//! net_taker_imbalance     = NetTakerImbalance(quote_volume, taker_buy_quote_volume)
//! log_total_quote         = ln(1 + qv_clipped)
//! value                   = ret_sign * close_delta^2 * net_taker_imbalance
//!                             / log_total_quote
//! ```
//!
//! Smoothing (two `ewm_mean`) and rolling min-max normalization live in
//! [`crate::harmony::ni_nor::NiNor`].

use std::fmt::Display;

use nautilus_model::data::BnBar;

use crate::{harmony::net_taker_imbalance::NetTakerImbalance, indicator::Indicator};

/// Denominator/log epsilon used by the notebook clip.
pub const DEFAULT_EPSILON: f64 = 1e-12;

/// Raw net imbalance factor (no smoothing). Binance-only input.
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
pub struct NetImbalance {
    pub epsilon: f64,
    pub value: f64,
    pub initialized: bool,
    prev_close: Option<f64>,
    net_taker_imbalance: NetTakerImbalance,
}

impl Display for NetImbalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl Indicator for NetImbalance {
    fn name(&self) -> String {
        stringify!(NetImbalance).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.prev_close.is_some()
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bn_bar(&mut self, bar: &BnBar) {
        self.update_raw(
            (&bar.close).into(),
            (&bar.quote_volume).into(),
            (&bar.taker_buy_quote_volume).into(),
        );
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.initialized = false;
        self.prev_close = None;
        self.net_taker_imbalance.reset();
    }
}

impl Default for NetImbalance {
    fn default() -> Self {
        Self::new(None)
    }
}

impl NetImbalance {
    /// Creates a new [`NetImbalance`] raw factor.
    ///
    /// `epsilon` defaults to [`DEFAULT_EPSILON`] (matching the notebook).
    #[must_use]
    pub fn new(epsilon: Option<f64>) -> Self {
        let epsilon = epsilon.unwrap_or(DEFAULT_EPSILON);
        Self {
            epsilon,
            value: 0.0,
            initialized: false,
            prev_close: None,
            net_taker_imbalance: NetTakerImbalance::new(Some(epsilon)),
        }
    }

    /// Updates the factor with raw numeric inputs.
    ///
    /// The first call only seeds `prev_close`; from the second call onward
    /// the raw factor value is produced and `initialized` becomes true.
    pub fn update_raw(
        &mut self,
        close: f64,
        quote_volume: f64,
        taker_buy_quote_volume: f64,
    ) {
        let Some(prev_close) = self.prev_close else {
            self.prev_close = Some(close);
            self.value = 0.0;
            self.initialized = false;
            return;
        };
        self.prev_close = Some(close);

        let close_delta = close - prev_close;
        let ret_sign = if close_delta > 0.0 {
            1.0
        } else if close_delta < 0.0 {
            -1.0
        } else {
            0.0
        };

        self.net_taker_imbalance
            .update_raw(quote_volume, taker_buy_quote_volume);
        let qv_clipped = quote_volume.max(self.epsilon);
        let log_total_quote = qv_clipped.ln_1p();

        self.value = if log_total_quote == 0.0 {
            0.0
        } else {
            ret_sign * close_delta.powi(2) * self.net_taker_imbalance.value / log_total_quote
        };
        self.initialized = true;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::NetImbalance;
    use crate::indicator::Indicator;

    #[rstest]
    fn test_display_and_default_not_initialized() {
        let ind = NetImbalance::default();
        assert_eq!(format!("{ind}"), "NetImbalance");
        assert!(!ind.initialized());
        assert!(!ind.has_inputs());
    }

    #[rstest]
    fn test_first_update_seeds_prev_close_only() {
        let mut ind = NetImbalance::default();
        ind.update_raw(100.0, 1_000.0, 600.0);
        assert!(ind.has_inputs());
        assert!(!ind.initialized());
        assert_eq!(ind.value, 0.0);
    }

    #[rstest]
    fn test_raw_matches_notebook_formula() {
        let mut ind = NetImbalance::default();
        ind.update_raw(100.0, 1_000.0, 600.0);

        let close = 101.0_f64;
        let qv = 1_500.0_f64;
        let taker_buy_qv = 900.0_f64;
        ind.update_raw(close, qv, taker_buy_qv);

        let close_delta = close - 100.0_f64;
        let sell_qv = qv - taker_buy_qv;
        let net_imb = (taker_buy_qv - sell_qv) / qv;
        let log_qv = qv.ln_1p();
        let expected = 1.0 * close_delta.powi(2) * net_imb / log_qv;

        assert!((ind.value - expected).abs() < 1e-12);
        assert!(ind.initialized());
    }

    #[rstest]
    fn test_negative_close_delta_sign() {
        let mut ind = NetImbalance::default();
        ind.update_raw(100.0, 1_000.0, 600.0);
        ind.update_raw(99.0, 1_500.0, 900.0);

        let close_delta = 99.0_f64 - 100.0;
        let sell_qv = 1_500.0 - 900.0;
        let net_imb = (900.0 - sell_qv) / 1_500.0;
        let expected = -1.0 * close_delta.powi(2) * net_imb / 1_500.0_f64.ln_1p();
        assert!((ind.value - expected).abs() < 1e-12);
    }

    #[rstest]
    fn test_reset() {
        let mut ind = NetImbalance::default();
        ind.update_raw(100.0, 1_000.0, 600.0);
        ind.update_raw(101.0, 1_500.0, 900.0);
        assert!(ind.initialized());
        ind.reset();
        assert!(!ind.initialized());
        assert!(!ind.has_inputs());
        assert_eq!(ind.value, 0.0);
    }
}
