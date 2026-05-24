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

//! Normalized [`NetImbalance`]: two `ewm_mean(adjust=True)` passes plus
//! rolling min-max mapping to `[-1, 1]`.
//!
//! Mirrors `crates/indicators/opt_factor.ipynb` end-to-end:
//!
//! ```text
//! raw_value      = NetImbalance(close, quote_volume, taker_buy_quote_volume)
//! smoothed_value = ewm_mean(ewm_mean(raw_value, alpha), alpha)
//! roll_min, roll_max  = rolling window over `smoothed_value` (length NORMALIZE_WINDOW)
//! value          = 2 * (smoothed_value - roll_min) / (roll_max - roll_min + eps) - 1
//! ```
//!
//! `initialized` becomes true once the rolling window is full.

use std::fmt::Display;

use arraydeque::{ArrayDeque, Wrapping};
use nautilus_model::data::BnBar;

use crate::{
    average::ewm::ExponentiallyWeightedMean,
    harmony::net_imbalance::NetImbalance,
    indicator::{Indicator, MovingAverage},
};

/// Alpha used by the two notebook `ewm_mean` passes.
pub const DEFAULT_EWM_ALPHA: f64 = 0.1;
/// Window used by the notebook rolling min-max normalization.
pub const DEFAULT_NORMALIZATION_PERIOD: usize = 1_400;
/// Denominator epsilon used by the notebook min-max scaling.
pub const DEFAULT_NORMALIZATION_EPSILON: f64 = 1e-12;
const MAX_NORMALIZATION_PERIOD: usize = 8_192;

/// Notebook normalized NetImbalance:
/// `NetImbalance -> ewm_mean(alpha) -> ewm_mean(alpha) -> rolling min-max [-1, 1]`.
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
pub struct NiNor {
    pub ewm_alpha: f64,
    pub normalization_period: usize,
    pub normalization_epsilon: f64,
    pub raw_value: f64,
    pub smoothed_value: f64,
    pub value: f64,
    pub initialized: bool,
    net_imbalance: NetImbalance,
    ewm1: ExponentiallyWeightedMean,
    ewm2: ExponentiallyWeightedMean,
    normalization_window: ArrayDeque<f64, MAX_NORMALIZATION_PERIOD, Wrapping>,
}

impl Display for NiNor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({}, {})",
            self.name(),
            self.ewm_alpha,
            self.normalization_period,
        )
    }
}

impl Indicator for NiNor {
    fn name(&self) -> String {
        stringify!(NiNor).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.net_imbalance.has_inputs()
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
        self.raw_value = 0.0;
        self.smoothed_value = 0.0;
        self.value = 0.0;
        self.initialized = false;
        self.net_imbalance.reset();
        self.ewm1.reset();
        self.ewm2.reset();
        self.normalization_window.clear();
    }
}

impl NiNor {
    /// Creates a new [`NiNor`] matching the notebook pipeline.
    ///
    /// # Panics
    ///
    /// Panics if `ewm_alpha` is not in `(0, 1]` or
    /// `normalization_period` is not in `(0, MAX_NORMALIZATION_PERIOD]`.
    #[must_use]
    pub fn new(ewm_alpha: Option<f64>, normalization_period: Option<usize>) -> Self {
        let ewm_alpha = ewm_alpha.unwrap_or(DEFAULT_EWM_ALPHA);
        assert!(
            ewm_alpha > 0.0 && ewm_alpha <= 1.0,
            "NiNor: ewm_alpha must be in (0, 1] (received {ewm_alpha})"
        );
        let normalization_period = normalization_period.unwrap_or(DEFAULT_NORMALIZATION_PERIOD);
        assert!(
            normalization_period > 0,
            "NiNor: normalization_period must be positive"
        );
        assert!(
            normalization_period <= MAX_NORMALIZATION_PERIOD,
            "NiNor: normalization_period {normalization_period} exceeds MAX_NORMALIZATION_PERIOD ({MAX_NORMALIZATION_PERIOD})"
        );

        Self {
            ewm_alpha,
            normalization_period,
            normalization_epsilon: DEFAULT_NORMALIZATION_EPSILON,
            raw_value: 0.0,
            smoothed_value: 0.0,
            value: 0.0,
            initialized: false,
            net_imbalance: NetImbalance::default(),
            ewm1: ExponentiallyWeightedMean::new(ewm_alpha, Some(1), None),
            ewm2: ExponentiallyWeightedMean::new(ewm_alpha, Some(1), None),
            normalization_window: ArrayDeque::new(),
        }
    }

    #[must_use]
    pub fn new_default() -> Self {
        Self::new(Some(DEFAULT_EWM_ALPHA), Some(DEFAULT_NORMALIZATION_PERIOD))
    }

    /// Updates the factor with raw numeric inputs.
    pub fn update_raw(
        &mut self,
        close: f64,
        quote_volume: f64,
        taker_buy_quote_volume: f64,
    ) {
        self.net_imbalance
            .update_raw(close, quote_volume, taker_buy_quote_volume);
        if !self.net_imbalance.initialized {
            self.raw_value = 0.0;
            self.smoothed_value = 0.0;
            self.value = 0.0;
            self.initialized = false;
            return;
        }

        self.raw_value = self.net_imbalance.value;
        self.ewm1.update_raw(self.raw_value);
        self.ewm2.update_raw(self.ewm1.value);
        self.smoothed_value = self.ewm2.value;

        if self.normalization_window.len() == self.normalization_period {
            self.normalization_window.pop_front();
        }
        let _ = self.normalization_window.push_back(self.smoothed_value);

        self.initialized = self.normalization_window.len() == self.normalization_period;
        if !self.initialized {
            self.value = 0.0;
            return;
        }

        let (min, max) = self.normalization_window.iter().fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(min, max), &value| (min.min(value), max.max(value)),
        );

        self.value =
            2.0 * (self.smoothed_value - min) / (max - min + self.normalization_epsilon) - 1.0;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::NiNor;
    use crate::indicator::Indicator;

    #[rstest]
    fn test_display_and_default_not_initialized() {
        let ind = NiNor::new_default();
        assert_eq!(format!("{ind}"), "NiNor(0.1, 1400)");
        assert!(!ind.initialized());
        assert!(!ind.has_inputs());
    }

    #[rstest]
    fn test_pipeline_initializes_after_normalization_window() {
        let mut ind = NiNor::new(Some(0.1), Some(5));

        // First call seeds prev_close, factor not initialized yet.
        ind.update_raw(100.0, 1_000.0, 600.0);
        assert!(!ind.initialized());
        assert!(ind.has_inputs());

        // After 4 more bars we have 4 raw-factor outputs in the window
        // but normalization_period=5, so still not initialized.
        for (i, qv) in [1_200.0, 1_500.0, 1_800.0, 2_100.0].iter().enumerate() {
            ind.update_raw(100.0 + (i as f64 + 1.0), *qv, *qv * 0.6);
            assert!(!ind.initialized());
        }

        // 5th raw-factor bar fills the window.
        ind.update_raw(110.0, 2_500.0, 1_500.0);
        assert!(ind.initialized());
        assert!(ind.value >= -1.0 && ind.value <= 1.0);
    }

    #[rstest]
    fn test_reset() {
        let mut ind = NiNor::new(Some(0.1), Some(3));
        for (i, qv) in [1_000.0, 1_200.0, 1_500.0, 1_800.0, 2_100.0]
            .iter()
            .enumerate()
        {
            ind.update_raw(100.0 + i as f64, *qv, *qv * 0.6);
        }
        assert!(ind.initialized());
        ind.reset();
        assert!(!ind.initialized());
        assert!(!ind.has_inputs());
        assert_eq!(ind.raw_value, 0.0);
        assert_eq!(ind.smoothed_value, 0.0);
        assert_eq!(ind.value, 0.0);
    }

    #[rstest]
    fn test_pipeline_matches_manual_calculation() {
        let alpha = 0.1_f64;
        let decay = 1.0 - alpha;
        let normalization_period = 4_usize;
        let mut ind = NiNor::new(Some(alpha), Some(normalization_period));

        let inputs: [(f64, f64, f64); 8] = [
            (100.0, 1_000.0, 600.0),
            (101.0, 1_500.0, 900.0),
            (102.0, 1_200.0, 500.0),
            (101.5, 800.0, 300.0),
            (103.0, 2_000.0, 1_500.0),
            (104.5, 2_300.0, 1_800.0),
            (103.5, 1_700.0, 600.0),
            (105.0, 2_600.0, 1_900.0),
        ];

        let mut prev_close: Option<f64> = None;
        let mut ws1 = 0.0_f64;
        let mut wn1 = 0.0_f64;
        let mut ws2 = 0.0_f64;
        let mut wn2 = 0.0_f64;
        let mut window: Vec<f64> = Vec::new();

        for (close, qv, taker_buy_qv) in inputs {
            ind.update_raw(close, qv, taker_buy_qv);

            let raw = match prev_close {
                None => {
                    prev_close = Some(close);
                    continue;
                }
                Some(prev) => {
                    let delta = close - prev;
                    let sign = if delta > 0.0 {
                        1.0
                    } else if delta < 0.0 {
                        -1.0
                    } else {
                        0.0
                    };
                    let sell_qv = qv - taker_buy_qv;
                    let net_imb = (taker_buy_qv - sell_qv) / qv;
                    let log_qv = qv.ln_1p();
                    let r = sign * delta.powi(2) * net_imb / log_qv;
                    prev_close = Some(close);
                    r
                }
            };

            ws1 = decay.mul_add(ws1, raw);
            wn1 = decay.mul_add(wn1, 1.0);
            let e1 = ws1 / wn1;

            ws2 = decay.mul_add(ws2, e1);
            wn2 = decay.mul_add(wn2, 1.0);
            let smoothed = ws2 / wn2;

            window.push(smoothed);
            if window.len() > normalization_period {
                window.remove(0);
            }

            assert!((ind.raw_value - raw).abs() < 1e-12);
            assert!((ind.smoothed_value - smoothed).abs() < 1e-12);

            if window.len() == normalization_period {
                let min = window.iter().cloned().fold(f64::INFINITY, f64::min);
                let max = window.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let expected =
                    2.0 * (smoothed - min) / (max - min + ind.normalization_epsilon) - 1.0;
                assert!(ind.initialized());
                assert!((ind.value - expected).abs() < 1e-12);
            }
        }
    }

    #[rstest]
    #[should_panic(expected = "ewm_alpha must be in (0, 1]")]
    fn test_zero_alpha_panics() {
        let _ = NiNor::new(Some(0.0), Some(10));
    }

    #[rstest]
    #[should_panic(expected = "normalization_period must be positive")]
    fn test_zero_window_panics() {
        let _ = NiNor::new(Some(0.1), Some(0));
    }

    #[rstest]
    #[should_panic(expected = "exceeds MAX_NORMALIZATION_PERIOD")]
    fn test_window_above_max_panics() {
        let _ = NiNor::new(Some(0.1), Some(10_000));
    }
}
