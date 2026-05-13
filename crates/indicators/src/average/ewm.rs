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

use std::fmt::Display;

use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::indicator::{Indicator, MovingAverage};

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
pub struct ExponentiallyWeightedMean {
    pub alpha: f64,
    pub min_samples: usize,
    pub price_type: PriceType,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    weighted_sum: f64,
    weight_sum: f64,
}

impl Display for ExponentiallyWeightedMean {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.alpha)
    }
}

impl Indicator for ExponentiallyWeightedMean {
    fn name(&self) -> String {
        stringify!(ExponentiallyWeightedMean).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.count > 0
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
        self.value = 0.0;
        self.count = 0;
        self.initialized = false;
        self.weighted_sum = 0.0;
        self.weight_sum = 0.0;
    }
}

impl ExponentiallyWeightedMean {
    /// Creates a new [`ExponentiallyWeightedMean`] instance matching Polars `ewm_mean`.
    ///
    /// Uses Polars defaults for `adjust=true` and `ignore_nulls=false` when inputs are finite raw
    /// values. The first output is available once `count >= min_samples`.
    ///
    /// # Panics
    ///
    /// Panics if `alpha` is not in `(0, 1]` or if `min_samples` is zero.
    #[must_use]
    pub fn new(alpha: f64, min_samples: Option<usize>, price_type: Option<PriceType>) -> Self {
        assert!(
            alpha > 0.0 && alpha <= 1.0,
            "ExponentiallyWeightedMean: alpha must be in (0, 1] (received {alpha})"
        );
        let min_samples = min_samples.unwrap_or(1);
        assert!(
            min_samples > 0,
            "ExponentiallyWeightedMean: min_samples must be > 0"
        );

        Self {
            alpha,
            min_samples,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            count: 0,
            initialized: false,
            weighted_sum: 0.0,
            weight_sum: 0.0,
        }
    }
}

impl MovingAverage for ExponentiallyWeightedMean {
    fn value(&self) -> f64 {
        self.value
    }

    fn count(&self) -> usize {
        self.count
    }

    fn update_raw(&mut self, value: f64) {
        let decay = 1.0 - self.alpha;
        self.weighted_sum = decay.mul_add(self.weighted_sum, value);
        self.weight_sum = decay.mul_add(self.weight_sum, 1.0);
        self.count += 1;
        self.initialized = self.count >= self.min_samples;

        if self.initialized {
            self.value = self.weighted_sum / self.weight_sum;
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::{Bar, QuoteTick, TradeTick},
        enums::PriceType,
    };
    use rstest::rstest;

    use crate::{
        average::ewm::ExponentiallyWeightedMean,
        indicator::{Indicator, MovingAverage},
        stubs::*,
    };

    #[rstest]
    fn test_ewm_initialized(indicator_ewm_alpha_10: ExponentiallyWeightedMean) {
        let ewm = indicator_ewm_alpha_10;
        assert_eq!(format!("{ewm}"), "ExponentiallyWeightedMean(0.1)");
        assert_eq!(ewm.alpha, 0.1);
        assert_eq!(ewm.min_samples, 1);
        assert_eq!(ewm.price_type, PriceType::Mid);
        assert!(!ewm.initialized());
        assert!(!ewm.has_inputs());
    }

    #[rstest]
    fn test_polars_adjust_true_values() {
        let mut ewm = ExponentiallyWeightedMean::new(0.5, Some(1), None);

        ewm.update_raw(1.0);
        assert_eq!(ewm.value, 1.0);
        ewm.update_raw(2.0);
        assert_eq!(ewm.value, 1.666_666_666_666_666_7);
        ewm.update_raw(3.0);
        assert_eq!(ewm.value, 2.428_571_428_571_428_4);
        ewm.update_raw(4.0);
        assert_eq!(ewm.value, 3.266_666_666_666_666_6);
    }

    #[rstest]
    fn test_notebook_alpha_point_one_values() {
        let mut ewm = ExponentiallyWeightedMean::new(0.1, Some(1), None);
        for value in [1.0, 2.0, 3.0, 4.0, 5.0] {
            ewm.update_raw(value);
        }
        assert!((ewm.value - 3.209_714_048_496_983_7).abs() < 1e-15);
    }

    #[rstest]
    fn test_min_samples_delays_initialization() {
        let mut ewm = ExponentiallyWeightedMean::new(0.5, Some(3), None);

        ewm.update_raw(1.0);
        ewm.update_raw(2.0);
        assert!(ewm.has_inputs());
        assert!(!ewm.initialized());
        assert_eq!(ewm.value, 0.0);

        ewm.update_raw(3.0);
        assert!(ewm.initialized());
        assert_eq!(ewm.value, 2.428_571_428_571_428_4);
    }

    #[rstest]
    fn test_reset(indicator_ewm_alpha_10: ExponentiallyWeightedMean) {
        let mut ewm = indicator_ewm_alpha_10;
        ewm.update_raw(1.0);
        assert_eq!(ewm.count, 1);
        ewm.reset();
        assert_eq!(ewm.count, 0);
        assert_eq!(ewm.value, 0.0);
        assert!(!ewm.initialized());
        assert!(!ewm.has_inputs());
    }

    #[rstest]
    fn test_handle_quote_tick_single(
        indicator_ewm_alpha_10: ExponentiallyWeightedMean,
        stub_quote: QuoteTick,
    ) {
        let mut ewm = indicator_ewm_alpha_10;
        ewm.handle_quote(&stub_quote);
        assert!(ewm.has_inputs());
        assert_eq!(ewm.value, 1501.0);
    }

    #[rstest]
    fn test_handle_trade_tick(
        indicator_ewm_alpha_10: ExponentiallyWeightedMean,
        stub_trade: TradeTick,
    ) {
        let mut ewm = indicator_ewm_alpha_10;
        ewm.handle_trade(&stub_trade);
        assert!(ewm.has_inputs());
        assert_eq!(ewm.value, 1500.0);
    }

    #[rstest]
    fn test_handle_bar(
        mut indicator_ewm_alpha_10: ExponentiallyWeightedMean,
        bar_ethusdt_binance_minute_bid: Bar,
    ) {
        indicator_ewm_alpha_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert!(indicator_ewm_alpha_10.has_inputs());
        assert!(indicator_ewm_alpha_10.initialized());
        assert_eq!(indicator_ewm_alpha_10.value, 1522.0);
    }

    #[rstest]
    #[should_panic(expected = "alpha must be in (0, 1]")]
    fn test_zero_alpha_panics() {
        let _ = ExponentiallyWeightedMean::new(0.0, None, None);
    }

    #[rstest]
    #[should_panic(expected = "alpha must be in (0, 1]")]
    fn test_alpha_above_one_panics() {
        let _ = ExponentiallyWeightedMean::new(1.1, None, None);
    }

    #[rstest]
    #[should_panic(expected = "min_samples must be > 0")]
    fn test_zero_min_samples_panics() {
        let _ = ExponentiallyWeightedMean::new(0.5, Some(0), None);
    }
}
