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

//! Parsing utilities for converting MEXC API responses into Nautilus domain models.

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{AggregationSource, BarAggregation, PriceType},
    identifiers::InstrumentId,
    instruments::InstrumentAny,
};

use super::models::{MexcKline, MexcTrade};

/// Parse result for instrument parsing.
#[derive(Debug)]
pub enum InstrumentParseResult {
    /// Successfully parsed instrument.
    Ok(Box<InstrumentAny>),
    /// Unsupported instrument type.
    Unsupported {
        symbol: String,
        instrument_type: String,
    },
    /// Inactive instrument.
    Inactive {
        symbol: String,
        reason: String,
    },
    /// Failed to parse instrument.
    Failed {
        symbol: String,
        instrument_type: String,
        error: String,
    },
}

/// Parses a MEXC trade into a Nautilus TradeTick.
///
/// # Errors
///
/// Returns an error if the trade data cannot be parsed.
pub fn parse_trade(
    trade: &MexcTrade,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = trade.price.parse::<f64>()?;
    let quantity = trade.quantity.parse::<f64>()?;
    let ts_event = trade
        .time
        .map(|t| UnixNanos::from(t * 1_000_000_000))
        .unwrap_or(ts_init);

    // Determine trade side based on is_buyer_maker
    // If is_buyer_maker is true, the buyer was the maker (passive), so the seller was the aggressor
    // In Nautilus, we track the aggressor side
    let aggressor_side = trade
        .is_buyer_maker
        .map(|is_maker| if is_maker { nautilus_model::enums::OrderSide::SELL } else { nautilus_model::enums::OrderSide::BUY })
        .unwrap_or(nautilus_model::enums::OrderSide::BUY);

    Ok(TradeTick::new(
        instrument_id,
        nautilus_model::types::Price::from(price),
        nautilus_model::types::Quantity::from(quantity),
        aggressor_side,
        trade.id.as_deref().unwrap_or("").to_string(),
        ts_event,
        ts_init,
    ))
}

/// Parses a MEXC kline into a Nautilus Bar.
///
/// # Errors
///
/// Returns an error if the kline data cannot be parsed.
pub fn parse_kline(
    kline: &MexcKline,
    instrument_id: InstrumentId,
    bar_type: BarType,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let open = kline.open.parse::<f64>()?;
    let high = kline.high.parse::<f64>()?;
    let low = kline.low.parse::<f64>()?;
    let close = kline.close.parse::<f64>()?;
    let volume = kline.volume.parse::<f64>()?;
    let quote_volume = kline
        .quote_volume
        .as_ref()
        .and_then(|v| v.parse::<f64>().ok());

    let ts_event = UnixNanos::from(kline.open_time * 1_000_000);
    let ts_init = ts_init;

    Ok(Bar::new(
        bar_type,
        nautilus_model::types::Price::from(open),
        nautilus_model::types::Price::from(high),
        nautilus_model::types::Price::from(low),
        nautilus_model::types::Price::from(close),
        nautilus_model::types::Quantity::from(volume),
        quote_volume.map(nautilus_model::types::Money::from),
        ts_event,
        ts_init,
    ))
}

/// Placeholder for parsing MEXC instruments.
/// TODO: Implement full instrument parsing based on MEXC API response format.
pub fn parse_instrument_any(
    _instrument: &super::models::MexcInstrument,
    _ts_init: UnixNanos,
) -> InstrumentParseResult {
    // TODO: Implement instrument parsing
    InstrumentParseResult::Unsupported {
        symbol: "TODO".to_string(),
        instrument_type: "TODO".to_string(),
    }
}


