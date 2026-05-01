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

//! Trend strength (directional efficiency) over a rolling window of closes.
//!
//! Specification (see repository `TrendStrengh因子/算法说明.md`):
//!
//! `factor_t = (C_t - C_{t-n}) / sum_{i=0}^{n-1} |C_{t-i} - C_{t-i-1}|`
//!
//! Default `n` is **120**. Use [`TrendStrength::new`] with a smaller `period` (e.g. **60**) to match
//! `TrendStrengh因子/main_crypto.ipynb`.

use std::{
    collections::VecDeque,
    fmt::Display,
};

use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::indicator::Indicator;

/// Default rolling window length from `TrendStrengh因子/算法说明.md` (`n = 120`).
pub const DEFAULT_PERIOD: usize = 120;

/// Rolling-window trend strength: signed net displacement divided by path length (sum of absolute
/// close-to-close moves).
#[repr(C)]
#[derive(Debug)]
pub struct TrendStrength {
    /// Window length `n` (uses `n + 1` closes: \\(C_{t-n}\\) through \\(C_t\\)).
    pub period: usize,
    pub price_type: PriceType,
    /// Latest factor value; zero until [`Self::initialized`] is true or when path sum is zero.
    pub value: f64,
    pub initialized: bool,
    closes: VecDeque<f64>,
    deltas: VecDeque<f64>,
    path_sum: f64,
}

impl Display for TrendStrength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for TrendStrength {
    fn name(&self) -> String {
        stringify!(TrendStrength).to_string()
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

impl TrendStrength {
    /// Creates a new [`TrendStrength`] with the given window length.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero.
    #[must_use]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        assert!(period > 0, "TrendStrength period must be positive");

        Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            initialized: false,
            closes: VecDeque::with_capacity(period + 1),
            deltas: VecDeque::with_capacity(period),
            path_sum: 0.0,
        }
    }

    /// Creates an instance with [`DEFAULT_PERIOD`] (120).
    #[must_use]
    pub fn new_default(price_type: Option<PriceType>) -> Self {
        Self::new(DEFAULT_PERIOD, price_type)
    }

    /// Updates the indicator with a raw close (or last) price as `f64`.
    pub fn update_raw(&mut self, close: f64) {
        if let Some(&prev) = self.closes.back() {
            let delta_abs = (close - prev).abs();
            self.path_sum += delta_abs;
            self.deltas.push_back(delta_abs);
            if self.deltas.len() > self.period {
                if let Some(old) = self.deltas.pop_front() {
                    self.path_sum -= old;
                }
            }
        }

        self.closes.push_back(close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }

        self.initialized = self.closes.len() == self.period + 1 && self.deltas.len() == self.period;

        if !self.initialized {
            self.value = 0.0;
            return;
        }

        let Some((&first, &last)) = self.closes.front().zip(self.closes.back()) else {
            self.value = 0.0;
            return;
        };
        let net_displacement = last - first;

        self.value = if self.path_sum == 0.0 {
            0.0
        } else {
            net_displacement / self.path_sum
        };
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{harmony::trend_strength::TrendStrength, indicator::Indicator, stubs::*};

    #[rstest]
    fn test_display_and_default_not_initialized() {
        let ind = TrendStrength::new_default(None);
        assert_eq!(format!("{ind}"), "TrendStrength(120)");
        assert!(!ind.initialized());
    }

    #[rstest]
    fn test_monotone_up_full_window() {
        let mut ind = TrendStrength::new(4, None);
        for x in [0.0_f64, 1.0, 2.0, 3.0, 4.0] {
            ind.update_raw(x);
        }
        assert!(ind.initialized());
        assert!((ind.value - 1.0).abs() < 1e-12);
    }

    #[rstest]
    fn test_monotone_down_full_window() {
        let mut ind = TrendStrength::new(4, None);
        for x in [4.0_f64, 3.0, 2.0, 1.0, 0.0] {
            ind.update_raw(x);
        }
        assert!(ind.initialized());
        assert!((ind.value - (-1.0)).abs() < 1e-12);
    }

    #[rstest]
    fn test_flat_path_zero() {
        let mut ind = TrendStrength::new(2, None);
        ind.update_raw(1.0);
        ind.update_raw(1.0);
        ind.update_raw(1.0);
        assert!(ind.initialized());
        assert_eq!(ind.value, 0.0);
    }

    #[rstest]
    fn test_reset() {
        let mut ind = TrendStrength::new(2, None);
        ind.update_raw(1.0);
        ind.update_raw(2.0);
        ind.update_raw(3.0);
        assert!(ind.initialized());
        ind.reset();
        assert!(!ind.initialized());
        assert_eq!(ind.value, 0.0);
    }

    #[rstest]
    fn test_handle_bar() {
        let mut ind = TrendStrength::new(2, None);
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
