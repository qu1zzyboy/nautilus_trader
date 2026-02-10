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

//! Fixed Donchian Channel implementation that only uses the last `period` bars.
//!
//! This is a corrected version of the Donchian Channel that maintains a sliding window
//! of exactly `period` elements, unlike the original implementation which keeps all
//! historical values up to MAX_PERIOD.

use std::fmt::Display;

use arraydeque::{ArrayDeque, Wrapping};
use nautilus_model::data::Bar;

use crate::indicator::Indicator;

const MAX_PERIOD: usize = 1_024;

/// Fixed Donchian Channel that only calculates based on the last `period` bars.
///
/// This implementation correctly maintains a sliding window of exactly `period` elements
/// by removing the oldest element when the window is full, ensuring that calculations
/// are based only on the most recent `period` bars rather than all historical data.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct FixedDonchianChannel {
    pub period: usize,
    pub upper: f64,
    pub middle: f64,
    pub lower: f64,
    pub initialized: bool,
    has_inputs: bool,
    upper_prices: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    lower_prices: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
}

impl Display for FixedDonchianChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FixedDonchianChannel({})", self.period)
    }
}

impl Indicator for FixedDonchianChannel {
    fn name(&self) -> String {
        "FixedDonchianChannel".to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.high).into(), (&bar.low).into());
    }

    fn reset(&mut self) {
        self.upper_prices.clear();
        self.lower_prices.clear();
        self.upper = 0.0;
        self.middle = 0.0;
        self.lower = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl FixedDonchianChannel {
    /// Creates a new [`FixedDonchianChannel`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `period` is not in the range of 1 to `MAX_PERIOD` (inclusive).
    #[must_use]
    pub fn new(period: usize) -> Self {
        assert!(
            period > 0 && period <= MAX_PERIOD,
            "FixedDonchianChannel: period {period} exceeds MAX_PERIOD ({MAX_PERIOD})"
        );

        Self {
            period,
            upper: 0.0,
            middle: 0.0,
            lower: 0.0,
            upper_prices: ArrayDeque::new(),
            lower_prices: ArrayDeque::new(),
            has_inputs: false,
            initialized: false,
        }
    }

    /// Updates the channel with new high and low values.
    ///
    /// This implementation maintains a sliding window of exactly `period` elements
    /// by removing the oldest element when the window is full. This ensures that
    /// calculations are based only on the most recent `period` bars, not all historical data.
    pub fn update_raw(&mut self, high: f64, low: f64) {
        // Maintain sliding window: remove oldest element if we have enough
        // This is the key fix: we only keep the last `period` elements
        if self.upper_prices.len() >= self.period {
            let _ = self.upper_prices.pop_front();
        }
        if self.lower_prices.len() >= self.period {
            let _ = self.lower_prices.pop_front();
        }

        // Add new values
        let _ = self.upper_prices.push_back(high);
        let _ = self.lower_prices.push_back(low);

        // Update initialization status
        if !self.initialized {
            self.has_inputs = true;
            if self.upper_prices.len() >= self.period && self.lower_prices.len() >= self.period {
                self.initialized = true;
            }
        }

        // Calculate upper and lower from the sliding window (only last `period` elements)
        // Since we maintain exactly `period` elements (or less before initialization),
        // we can iterate all of them
        self.upper = self
            .upper_prices
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        self.lower = self
            .lower_prices
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        self.middle = 0.5 * (self.upper + self.lower);
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::Bar;
    use rstest::{fixture, rstest};

    use crate::{
        indicator::Indicator,
        stubs::bar_ethusdt_binance_minute_bid,
        volatility::dc_fixed::FixedDonchianChannel,
    };

    #[fixture]
    fn fixed_dc_10() -> FixedDonchianChannel {
        FixedDonchianChannel::new(10)
    }

    #[rstest]
    fn test_fixed_dc_initialized(fixed_dc_10: FixedDonchianChannel) {
        let display_str = format!("{fixed_dc_10}");
        assert_eq!(display_str, "FixedDonchianChannel(10)");
        assert_eq!(fixed_dc_10.period, 10);
        assert!(!fixed_dc_10.initialized);
        assert!(!fixed_dc_10.has_inputs);
    }

    #[rstest]
    fn test_fixed_dc_value_with_one_input(mut fixed_dc_10: FixedDonchianChannel) {
        fixed_dc_10.update_raw(1.0, 0.9);
        assert_eq!(fixed_dc_10.upper, 1.0);
        assert_eq!(fixed_dc_10.middle, 0.95);
        assert_eq!(fixed_dc_10.lower, 0.9);
    }

    #[rstest]
    fn test_fixed_dc_value_with_three_inputs(mut fixed_dc_10: FixedDonchianChannel) {
        fixed_dc_10.update_raw(1.0, 0.9);
        fixed_dc_10.update_raw(2.0, 1.8);
        fixed_dc_10.update_raw(3.0, 2.7);
        assert_eq!(fixed_dc_10.upper, 3.0);
        assert_eq!(fixed_dc_10.middle, 1.95);
        assert_eq!(fixed_dc_10.lower, 0.9);
    }

    #[rstest]
    fn test_fixed_dc_sliding_window(mut fixed_dc_10: FixedDonchianChannel) {
        // Add 15 values, but should only use last 10
        let high_values = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];
        let low_values = [
            0.9, 1.9, 2.9, 3.9, 4.9, 5.9, 6.9, 7.9, 8.9, 9.9, 10.1, 10.2, 10.3, 11.1, 11.4,
        ];

        for i in 0..15 {
            fixed_dc_10.update_raw(high_values[i], low_values[i]);
        }

        // Should only use last 10 values: [6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0]
        // Upper should be 15.0 (max of last 10)
        // Lower should be 5.9 (min of last 10: [5.9, 6.9, 7.9, 8.9, 9.9, 10.1, 10.2, 10.3, 11.1, 11.4])
        assert_eq!(fixed_dc_10.upper, 15.0);
        assert_eq!(fixed_dc_10.lower, 5.9);
        assert_eq!(fixed_dc_10.middle, 10.45);
        assert!(fixed_dc_10.initialized);
    }

    #[rstest]
    fn test_fixed_dc_handle_bar(mut fixed_dc_10: FixedDonchianChannel, bar_ethusdt_binance_minute_bid: Bar) {
        fixed_dc_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(fixed_dc_10.upper, 1550.0);
        assert_eq!(fixed_dc_10.middle, 1522.5);
        assert_eq!(fixed_dc_10.lower, 1495.0);
        assert!(fixed_dc_10.has_inputs);
        assert!(!fixed_dc_10.initialized);
    }

    #[rstest]
    fn test_fixed_dc_reset(mut fixed_dc_10: FixedDonchianChannel) {
        fixed_dc_10.update_raw(1.0, 0.9);
        fixed_dc_10.reset();
        assert_eq!(fixed_dc_10.upper_prices.len(), 0);
        assert_eq!(fixed_dc_10.lower_prices.len(), 0);
        assert_eq!(fixed_dc_10.upper, 0.0);
        assert_eq!(fixed_dc_10.middle, 0.0);
        assert_eq!(fixed_dc_10.lower, 0.0);
        assert!(!fixed_dc_10.has_inputs);
        assert!(!fixed_dc_10.initialized);
    }
}

