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

//! Complete trend strength factor from `sync/TrendStrengh因子/main_crypto.ipynb`.

use std::{
    collections::VecDeque,
    fmt::Display,
};

use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::{
    average::ewm::ExponentiallyWeightedMean,
    harmony::pl2dist::{Pl2Dist, DEFAULT_PERIOD},
    indicator::{Indicator, MovingAverage},
};

/// Alpha used by the two notebook `ewm_mean` passes.
pub const DEFAULT_EWM_ALPHA: f64 = 0.1;
/// Window used by the notebook rolling min-max normalization.
pub const DEFAULT_NORMALIZATION_PERIOD: usize = 5_000;
/// Denominator epsilon used by the notebook min-max scaling.
pub const DEFAULT_NORMALIZATION_EPSILON: f64 = 1e-10;

/// Complete notebook trend strength factor:
/// `Pl2Dist -> ewm_mean(alpha=0.1) -> ewm_mean(alpha=0.1) -> rolling min-max [-1, 1]`.
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
pub struct TrendStrength {
    pub period: usize,
    pub ewm_alpha: f64,
    pub normalization_period: usize,
    pub normalization_epsilon: f64,
    pub price_type: PriceType,
    pub raw_value: f64,
    pub smoothed_value: f64,
    pub value: f64,
    pub initialized: bool,
    pl2dist: Pl2Dist,
    ewm1: ExponentiallyWeightedMean,
    ewm2: ExponentiallyWeightedMean,
    normalization_window: VecDeque<f64>,
}

impl Display for TrendStrength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({}, {}, {})",
            self.name(),
            self.period,
            self.ewm_alpha,
            self.normalization_period
        )
    }
}

impl Indicator for TrendStrength {
    fn name(&self) -> String {
        stringify!(TrendStrength).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.pl2dist.has_inputs()
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, quote: &QuoteTick) {
        self.update_raw(quote.extract_price(self.price_type).into());
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        self.update_raw((&trade.price).into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.raw_value = 0.0;
        self.smoothed_value = 0.0;
        self.value = 0.0;
        self.initialized = false;
        self.pl2dist.reset();
        self.ewm1.reset();
        self.ewm2.reset();
        self.normalization_window.clear();
    }
}

impl TrendStrength {
    /// Creates a new [`TrendStrength`] matching the notebook pipeline.
    ///
    /// # Panics
    ///
    /// Panics if `period`, `normalization_period`, or `ewm_alpha` are invalid.
    #[must_use]
    pub fn new(
        period: usize,
        price_type: Option<PriceType>,
        ewm_alpha: Option<f64>,
        normalization_period: Option<usize>,
    ) -> Self {
        let ewm_alpha = ewm_alpha.unwrap_or(DEFAULT_EWM_ALPHA);
        let normalization_period = normalization_period.unwrap_or(DEFAULT_NORMALIZATION_PERIOD);
        assert!(
            normalization_period > 0,
            "TrendStrength normalization_period must be positive"
        );

        let price_type = price_type.unwrap_or(PriceType::Last);
        Self {
            period,
            ewm_alpha,
            normalization_period,
            normalization_epsilon: DEFAULT_NORMALIZATION_EPSILON,
            price_type,
            raw_value: 0.0,
            smoothed_value: 0.0,
            value: 0.0,
            initialized: false,
            pl2dist: Pl2Dist::new(period, Some(price_type)),
            ewm1: ExponentiallyWeightedMean::new(ewm_alpha, Some(1), Some(price_type)),
            ewm2: ExponentiallyWeightedMean::new(ewm_alpha, Some(1), Some(price_type)),
            normalization_window: VecDeque::with_capacity(normalization_period),
        }
    }

    #[must_use]
    pub fn new_default(price_type: Option<PriceType>) -> Self {
        Self::new(
            DEFAULT_PERIOD,
            price_type,
            Some(DEFAULT_EWM_ALPHA),
            Some(DEFAULT_NORMALIZATION_PERIOD),
        )
    }

    pub fn update_raw(&mut self, close: f64) {
        self.pl2dist.update_raw(close);
        if !self.pl2dist.initialized {
            self.raw_value = 0.0;
            self.smoothed_value = 0.0;
            self.value = 0.0;
            self.initialized = false;
            return;
        }

        self.raw_value = self.pl2dist.value;
        self.ewm1.update_raw(self.raw_value);
        self.ewm2.update_raw(self.ewm1.value);
        self.smoothed_value = self.ewm2.value;

        self.normalization_window.push_back(self.smoothed_value);
        if self.normalization_window.len() > self.normalization_period {
            self.normalization_window.pop_front();
        }

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

    use crate::{harmony::trend_strength::TrendStrength, indicator::Indicator};

    #[rstest]
    fn test_pipeline_initializes_after_normalization_window() {
        let mut ind = TrendStrength::new(2, None, Some(0.1), Some(5));
        ind.update_raw(1.0);
        ind.update_raw(2.0);
        assert!(!ind.initialized());

        ind.update_raw(3.0);
        assert!(!ind.initialized());
        assert_eq!(ind.raw_value, 1.0);
        assert_eq!(ind.smoothed_value, 1.0);
        assert_eq!(ind.value, 0.0);

        for value in [4.0, 5.0, 6.0, 7.0] {
            ind.update_raw(value);
        }
        assert!(ind.initialized());
    }

    #[rstest]
    fn test_reset() {
        let mut ind = TrendStrength::new(2, None, Some(0.1), Some(5));
        for value in [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0] {
            ind.update_raw(value);
        }
        assert!(ind.initialized());
        ind.reset();
        assert!(!ind.initialized());
        assert_eq!(ind.value, 0.0);
    }
}
