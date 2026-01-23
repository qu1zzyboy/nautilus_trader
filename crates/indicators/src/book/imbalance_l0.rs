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

//! Order book level 0 imbalance indicator.
//!
//! Calculates the imbalance ratio using only the best bid and ask quantities:
//! imbalance_l0 = (bid_amount - ask_amount) / (bid_amount + ask_amount)
//!
//! Returns a value in the range [-1.0, 1.0]:
//! - Positive values indicate buy pressure (more bid volume)
//! - Negative values indicate sell pressure (more ask volume)
//! - Zero indicates balanced order book

use std::fmt::Display;

use nautilus_model::{orderbook::OrderBook, types::Quantity};

use crate::indicator::Indicator;

/// Epsilon value for floating point comparison (1e-10).
const EPSILON: f64 = 1e-10;

#[repr(C)]
#[derive(Debug, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct BookImbalanceLevel0 {
    /// The current imbalance value in range [-1.0, 1.0].
    pub value: f64,
    /// The number of updates processed.
    pub count: usize,
    /// Whether the indicator has been initialized with valid data.
    pub initialized: bool,
    has_inputs: bool,
}

impl Display for BookImbalanceLevel0 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}()", self.name())
    }
}

impl Indicator for BookImbalanceLevel0 {
    fn name(&self) -> String {
        stringify!(BookImbalanceLevel0).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_book(&mut self, book: &OrderBook) {
        // Get the total size at the best bid/ask level (level 0)
        // For MBP books, we need the total size of all orders at the best price level
        // best_bid_size() only returns the first order's size, so we use level.size() instead
        // We need to get the precision from the first order to maintain accuracy
        let bid_qty = book
            .bids(None)
            .next()
            .and_then(|level| {
                // Get total size of all orders at this level
                let total_size = level.size();
                // Get precision from the first order (if any) or use 0 as default
                let precision = level.first().map(|order| order.size.precision).unwrap_or(0);
                Some(Quantity::new(total_size, precision))
            });
        let ask_qty = book
            .asks(None)
            .next()
            .and_then(|level| {
                let total_size = level.size();
                let precision = level.first().map(|order| order.size.precision).unwrap_or(0);
                Some(Quantity::new(total_size, precision))
            });
        
        self.update(bid_qty, ask_qty);
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl BookImbalanceLevel0 {
    /// Creates a new [`BookImbalanceLevel0`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            value: 0.0,
            count: 0,
            has_inputs: false,
            initialized: false,
        }
    }

    /// Updates the indicator with new bid and ask quantities.
    ///
    /// # Arguments
    ///
    /// * `best_bid` - The quantity at the best bid price (bids[0].amount)
    /// * `best_ask` - The quantity at the best ask price (asks[0].amount)
    ///
    /// # Formula
    ///
    /// imbalance_l0 = (bid_amount - ask_amount) / (bid_amount + ask_amount)
    ///
    /// Returns 0.0 if total_depth < epsilon to avoid division by zero.
    pub fn update(&mut self, best_bid: Option<Quantity>, best_ask: Option<Quantity>) {
        self.has_inputs = true;
        self.count += 1;

        if let (Some(bid_amount), Some(ask_amount)) = (best_bid, best_ask) {
            let bid_f64 = bid_amount.as_f64();
            let ask_f64 = ask_amount.as_f64();

            // Calculate total depth
            let total_depth = bid_f64 + ask_f64;

            // Boundary check: if total_depth < epsilon, return 0.0
            if total_depth < EPSILON {
                self.value = 0.0;
                self.initialized = true;
                return;
            }

            // Calculate imbalance: (bid_amount - ask_amount) / total_depth
            // Range: [-1.0, 1.0]
            // Positive: buy pressure (more bid volume)
            // Negative: sell pressure (more ask volume)
            // Zero: balanced
            self.value = (bid_f64 - ask_f64) / total_depth;
            self.initialized = true;
        } else {
            // No market yet - keep previous value or 0.0
            self.initialized = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        identifiers::InstrumentId,
        stubs::{stub_order_book_mbp, stub_order_book_mbp_appl_xnas},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_initialized() {
        let imbalance = BookImbalanceLevel0::new();
        let display_str = format!("{imbalance}");
        assert_eq!(display_str, "BookImbalanceLevel0()");
        assert_eq!(imbalance.value, 0.0);
        assert_eq!(imbalance.count, 0);
        assert!(!imbalance.has_inputs);
        assert!(!imbalance.initialized);
    }

    #[rstest]
    fn test_balanced_order_book() {
        let mut imbalance = BookImbalanceLevel0::new();
        // bid_amount = 100, ask_amount = 100
        // imbalance = (100 - 100) / (100 + 100) = 0 / 200 = 0.0
        let book = stub_order_book_mbp(
            InstrumentId::from("AAPL.XNAS"),
            101.0,
            100.0,
            100.0, // bid_amount
            100.0, // ask_amount
            2,
            0.01,
            0,
            100.0,
            10,
        );
        imbalance.handle_book(&book);

        assert_eq!(imbalance.count, 1);
        assert!((imbalance.value - 0.0).abs() < 1e-10);
        assert!(imbalance.initialized);
        assert!(imbalance.has_inputs);
    }

    #[rstest]
    fn test_buy_pressure() {
        let mut imbalance = BookImbalanceLevel0::new();
        // bid_amount = 200, ask_amount = 100
        // imbalance = (200 - 100) / (200 + 100) = 100 / 300 = 0.333...
        // Note: stub_order_book_mbp parameter order is:
        // top_ask_price, top_bid_price, top_ask_size, top_bid_size
        let book = stub_order_book_mbp(
            InstrumentId::from("AAPL.XNAS"),
            101.0,  // top_ask_price
            100.0,  // top_bid_price
            100.0,  // top_ask_size
            200.0,  // top_bid_size (larger)
            2,
            0.01,
            0,     // size_precision = 0
            100.0, // size_increment
            10,    // num_levels
        );
        
        imbalance.handle_book(&book);

        assert_eq!(imbalance.count, 1);
        let expected = (200.0 - 100.0) / (200.0 + 100.0); // 0.333...
        assert!((imbalance.value - expected).abs() < 1e-10);
        assert!(imbalance.value > 0.0); // Positive = buy pressure
        assert!(imbalance.initialized);
        assert!(imbalance.has_inputs);
    }

    #[rstest]
    fn test_sell_pressure() {
        let mut imbalance = BookImbalanceLevel0::new();
        // bid_amount = 100, ask_amount = 200
        // imbalance = (100 - 200) / (100 + 200) = -100 / 300 = -0.333...
        let book = stub_order_book_mbp(
            InstrumentId::from("AAPL.XNAS"),
            101.0,  // top_ask_price
            100.0,  // top_bid_price
            200.0,  // top_ask_size (larger)
            100.0,  // top_bid_size
            2,
            0.01,
            0,
            100.0,
            10,
        );
        imbalance.handle_book(&book);

        assert_eq!(imbalance.count, 1);
        let expected = (100.0 - 200.0) / (100.0 + 200.0); // -0.333...
        assert!((imbalance.value - expected).abs() < 1e-10);
        assert!(imbalance.value < 0.0); // Negative = sell pressure
        assert!(imbalance.initialized);
        assert!(imbalance.has_inputs);
    }

    #[rstest]
    fn test_extreme_buy_pressure() {
        let mut imbalance = BookImbalanceLevel0::new();
        // bid_amount = 1000, ask_amount = 1
        // imbalance = (1000 - 1) / (1000 + 1) ≈ 0.999
        let book = stub_order_book_mbp(
            InstrumentId::from("AAPL.XNAS"),
            101.0,  // top_ask_price
            100.0,  // top_bid_price
            1.0,    // top_ask_size (very small)
            1000.0, // top_bid_size (much larger)
            2,
            0.01,
            0,
            100.0,
            10,
        );
        imbalance.handle_book(&book);

        assert_eq!(imbalance.count, 1);
        let expected = (1000.0 - 1.0) / (1000.0 + 1.0);
        assert!((imbalance.value - expected).abs() < 1e-10);
        assert!(imbalance.value > 0.9); // Close to 1.0
        assert!(imbalance.initialized);
    }

    #[rstest]
    fn test_extreme_sell_pressure() {
        let mut imbalance = BookImbalanceLevel0::new();
        // bid_amount = 1, ask_amount = 1000
        // imbalance = (1 - 1000) / (1 + 1000) ≈ -0.999
        let book = stub_order_book_mbp(
            InstrumentId::from("AAPL.XNAS"),
            101.0,  // top_ask_price
            100.0,  // top_bid_price
            1000.0, // top_ask_size (much larger)
            1.0,    // top_bid_size (very small)
            2,
            0.01,
            0,
            100.0,
            10,
        );
        imbalance.handle_book(&book);

        assert_eq!(imbalance.count, 1);
        let expected = (1.0 - 1000.0) / (1.0 + 1000.0);
        assert!((imbalance.value - expected).abs() < 1e-10);
        assert!(imbalance.value < -0.9); // Close to -1.0
        assert!(imbalance.initialized);
    }

    #[rstest]
    fn test_zero_depth() {
        let mut imbalance = BookImbalanceLevel0::new();
        // Both bid and ask are 0 or very small
        // Should return 0.0 to avoid division by zero
        let book = stub_order_book_mbp(
            InstrumentId::from("AAPL.XNAS"),
            101.0,
            100.0,
            0.0, // bid_amount = 0
            0.0, // ask_amount = 0
            2,
            0.01,
            0,
            100.0,
            10,
        );
        imbalance.handle_book(&book);

        assert_eq!(imbalance.count, 1);
        assert!((imbalance.value - 0.0).abs() < 1e-10);
        assert!(imbalance.initialized);
    }

    #[rstest]
    fn test_reset() {
        let mut imbalance = BookImbalanceLevel0::new();
        let book = stub_order_book_mbp_appl_xnas();
        imbalance.handle_book(&book);
        imbalance.reset();

        assert_eq!(imbalance.count, 0);
        assert_eq!(imbalance.value, 0.0);
        assert!(!imbalance.initialized);
        assert!(!imbalance.has_inputs);
    }

    #[rstest]
    fn test_multiple_updates() {
        let mut imbalance = BookImbalanceLevel0::new();
        let book1 = stub_order_book_mbp(
            InstrumentId::from("AAPL.XNAS"),
            101.0,  // top_ask_price
            100.0,  // top_bid_price
            100.0,  // top_ask_size
            200.0,  // top_bid_size (larger)
            2,
            0.01,
            0,
            100.0,
            10,
        );
        imbalance.handle_book(&book1);
        let value1 = imbalance.value;

        let book2 = stub_order_book_mbp(
            InstrumentId::from("AAPL.XNAS"),
            101.0,  // top_ask_price
            100.0,  // top_bid_price
            200.0,  // top_ask_size (larger)
            100.0,  // top_bid_size
            2,
            0.01,
            0,
            100.0,
            10,
        );
        imbalance.handle_book(&book2);
        let value2 = imbalance.value;

        assert_eq!(imbalance.count, 2);
        assert!(value1 > 0.0); // First update: buy pressure
        assert!(value2 < 0.0); // Second update: sell pressure
        assert_eq!(value1, -value2); // Should be symmetric
    }
}

