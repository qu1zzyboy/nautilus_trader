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

use std::str::FromStr;

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::AggressorSide,
    identifiers::{InstrumentId, TradeId},
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
    // Parse price and quantity from strings
    let price = nautilus_model::types::Price::from_str(&trade.price)
        .map_err(|e| anyhow::anyhow!("Failed to parse price '{}': {}", trade.price, e))?;
    let quantity = nautilus_model::types::Quantity::from_str(&trade.quantity)
        .map_err(|e| anyhow::anyhow!("Failed to parse quantity '{}': {}", trade.quantity, e))?;
    
    let ts_event = trade
        .time
        .map(|t| UnixNanos::from((t as u64) * 1_000_000_000))
        .unwrap_or(ts_init);

    // Determine aggressor side based on is_buyer_maker
    // If is_buyer_maker is true, the buyer was the maker (passive), so the seller was the aggressor
    // In Nautilus, we track the aggressor side
    let aggressor_side = trade
        .is_buyer_maker
        .map(|is_maker| {
            if is_maker {
                AggressorSide::Seller
            } else {
                AggressorSide::Buyer
            }
        })
        .unwrap_or(AggressorSide::Buyer);

    let trade_id = TradeId::new_checked(
        trade.id.as_deref().unwrap_or("").to_string(),
    )
    .map_err(|e| anyhow::anyhow!("Invalid trade ID: {}", e))?;

    Ok(TradeTick::new(
        instrument_id,
        price,
        quantity,
        aggressor_side,
        trade_id,
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
    _instrument_id: InstrumentId,
    bar_type: BarType,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    // Parse prices and volume from strings
    let open = nautilus_model::types::Price::from_str(&kline.open)
        .map_err(|e| anyhow::anyhow!("Failed to parse open price '{}': {}", kline.open, e))?;
    let high = nautilus_model::types::Price::from_str(&kline.high)
        .map_err(|e| anyhow::anyhow!("Failed to parse high price '{}': {}", kline.high, e))?;
    let low = nautilus_model::types::Price::from_str(&kline.low)
        .map_err(|e| anyhow::anyhow!("Failed to parse low price '{}': {}", kline.low, e))?;
    let close = nautilus_model::types::Price::from_str(&kline.close)
        .map_err(|e| anyhow::anyhow!("Failed to parse close price '{}': {}", kline.close, e))?;
    let volume = nautilus_model::types::Quantity::from_str(&kline.volume)
        .map_err(|e| anyhow::anyhow!("Failed to parse volume '{}': {}", kline.volume, e))?;

    // Convert milliseconds to nanoseconds
    let ts_event = UnixNanos::from((kline.open_time as u64) * 1_000_000);

    Ok(Bar::new(
        bar_type,
        open,
        high,
        low,
        close,
        volume,
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


