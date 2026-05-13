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

//! Raw price-location-to-distance ratio: displacement divided by path length.

use std::fmt::Display;

use arraydeque::{ArrayDeque, Wrapping};
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::indicator::Indicator;

/// Window used by the notebook (`WINDOW = 60`).
pub const DEFAULT_PERIOD: usize = 60;
const MAX_PERIOD: usize = 8_192;
const MAX_CLOSES: usize = MAX_PERIOD + 1;

/// Raw price-location-to-distance ratio:
/// `(C_t - C_{t-n}) / sum(|C_i - C_{i-1}|)`.
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
pub struct Pl2Dist {
    pub period: usize,
    pub price_type: PriceType,
    pub value: f64,
    pub initialized: bool,
    closes: ArrayDeque<f64, MAX_CLOSES, Wrapping>,
    deltas: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    path_sum: f64,
}

impl Display for Pl2Dist {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for Pl2Dist {
    fn name(&self) -> String {
        stringify!(Pl2Dist).to_string()
    }

    fn has_inputs(&self) -> bool {
        !self.closes.is_empty()
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
        self.initialized = false;
        self.closes.clear();
        self.deltas.clear();
        self.path_sum = 0.0;
    }
}

impl Pl2Dist {
    /// Creates a new [`Pl2Dist`] with the given window length.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero.
    #[must_use]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        assert!(period > 0, "Pl2Dist period must be positive");
        assert!(
            period <= MAX_PERIOD,
            "Pl2Dist period {period} exceeds MAX_PERIOD ({MAX_PERIOD})"
        );

        Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            initialized: false,
            closes: ArrayDeque::new(),
            deltas: ArrayDeque::new(),
            path_sum: 0.0,
        }
    }

    #[must_use]
    pub fn new_default(price_type: Option<PriceType>) -> Self {
        Self::new(DEFAULT_PERIOD, price_type)
    }

    pub fn update_raw(&mut self, close: f64) {
        if let Some(&prev) = self.closes.back() {
            let delta_abs = (close - prev).abs();
            if self.deltas.len() == self.period {
                let old = self.deltas.pop_front().expect("deltas must be non-empty");
                self.path_sum -= old;
            }
            let _ = self.deltas.push_back(delta_abs);
            self.path_sum += delta_abs;
        }

        if self.closes.len() == self.period + 1 {
            self.closes.pop_front();
        }
        let _ = self.closes.push_back(close);

        self.initialized = self.closes.len() == self.period + 1 && self.deltas.len() == self.period;
        if !self.initialized {
            self.value = 0.0;
            return;
        }

        let Some((&first, &last)) = self.closes.front().zip(self.closes.back()) else {
            self.value = 0.0;
            return;
        };

        self.value = if self.path_sum == 0.0 {
            0.0
        } else {
            (last - first) / self.path_sum
        };
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{harmony::pl2dist::Pl2Dist, indicator::Indicator, stubs::*};

    #[rstest]
    fn test_display_and_default_not_initialized() {
        let ind = Pl2Dist::new_default(None);
        assert_eq!(format!("{ind}"), "Pl2Dist(60)");
        assert!(!ind.initialized());
    }

    #[rstest]
    fn test_monotone_up_full_window() {
        let mut ind = Pl2Dist::new(4, None);
        for x in [0.0_f64, 1.0, 2.0, 3.0, 4.0] {
            ind.update_raw(x);
        }
        assert!(ind.initialized());
        assert!((ind.value - 1.0).abs() < 1e-12);
    }

    #[rstest]
    fn test_monotone_down_full_window() {
        let mut ind = Pl2Dist::new(4, None);
        for x in [4.0_f64, 3.0, 2.0, 1.0, 0.0] {
            ind.update_raw(x);
        }
        assert!(ind.initialized());
        assert!((ind.value - (-1.0)).abs() < 1e-12);
    }

    #[rstest]
    fn test_flat_path_zero() {
        let mut ind = Pl2Dist::new(2, None);
        ind.update_raw(1.0);
        ind.update_raw(1.0);
        ind.update_raw(1.0);
        assert!(ind.initialized());
        assert_eq!(ind.value, 0.0);
    }

    #[rstest]
    fn test_handle_bar() {
        let mut ind = Pl2Dist::new(2, None);
        let bar1 = bar_ethusdt_binance_minute_bid("100.0");
        let bar2 = bar_ethusdt_binance_minute_bid("101.0");
        let bar3 = bar_ethusdt_binance_minute_bid("103.0");
        ind.handle_bar(&bar1);
        ind.handle_bar(&bar2);
        ind.handle_bar(&bar3);
        assert!(ind.initialized());
        assert!((ind.value - 1.0).abs() < 1e-9);
    }
}
