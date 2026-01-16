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

use anyhow::Context;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::AggressorSide,
    identifiers::{InstrumentId, Symbol, TradeId, Venue},
    instruments::{currency_pair::CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::common::{consts::MEXC_VENUE, enums::MexcSymbolStatus};
use super::models::{MexcInstrument, MexcKline, MexcTrade};

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

/// Returns a currency from the internal map or creates a new crypto currency.
fn get_currency(code: &str) -> Currency {
    Currency::get_or_create_crypto(code)
}

/// Parses a MEXC instrument into a Nautilus CurrencyPair instrument.
///
/// # Errors
///
/// Returns an error if:
/// - The instrument status is not TRADING.
/// - Required fields (base_currency, quote_currency, tick_size, min_quantity) are missing.
/// - Price or quantity values cannot be parsed.
pub fn parse_instrument_any(
    instrument: &MexcInstrument,
    ts_init: UnixNanos,
) -> InstrumentParseResult {
    let symbol_str = instrument.symbol.as_str();

    // Check status - only parse TRADING instruments
    // MEXC API returns status as "1" (string) for enabled/trading instruments
    // Status values: "1" = enabled/trading, "0" = disabled/halted
    let status = instrument.status.as_deref().unwrap_or("");
    if status != "1" && status != "ENABLED" && status != "TRADING" && status != MexcSymbolStatus::Trading.as_ref() {
        return InstrumentParseResult::Inactive {
            symbol: symbol_str.to_string(),
            reason: format!("Status is not enabled/trading (got: {})", status),
        };
    }

    // Extract base and quote currencies
    let (base_code, quote_code) = match (
        instrument.base_currency.as_ref(),
        instrument.quote_currency.as_ref(),
    ) {
        (Some(base), Some(quote)) => (base.as_str(), quote.as_str()),
        _ => {
            // Try to extract from symbol (e.g., "BTCUSDT" -> base="BTC", quote="USDT")
            // This is a fallback - ideally API should provide these fields
            // Common quote currencies: USDT, USD, BUSD, USDC, BTC, ETH, EUR, BRL, etc.
            // Note: Order matters - longer matches first (e.g., "USDT" before "USD")
            let quote_candidates = [
                "USDT", "USDC", "BUSD", "USDE", "USDF", "USD1", // USD variants
                "EUR", "BRL", "GBP", "JPY", "KRW", // Fiat currencies
                "BTC", "ETH", "BNB", // Major crypto
            ];
            let mut found = false;
            let mut base_code = "";
            let mut quote_code = "";

            for quote in &quote_candidates {
                if symbol_str.ends_with(quote) && symbol_str.len() > quote.len() {
                    base_code = &symbol_str[..symbol_str.len() - quote.len()];
                    quote_code = quote;
                    found = true;
                    break;
                }
            }

            if !found {
                return InstrumentParseResult::Failed {
                    symbol: symbol_str.to_string(),
                    instrument_type: "CurrencyPair".to_string(),
                    error: format!("Cannot extract base/quote currencies from symbol '{}' and API fields are missing", symbol_str),
                };
            }

            (base_code, quote_code)
        }
    };

    let base_currency = get_currency(base_code);
    let quote_currency = get_currency(quote_code);

    // Create instrument ID
    let instrument_id = InstrumentId::new(
        Symbol::from_str_unchecked(symbol_str),
        *MEXC_VENUE,
    );
    let raw_symbol = Symbol::new(symbol_str);

    // Parse tick size (minimum price increment)
    let tick_size = match &instrument.tick_size {
        Some(ts) => {
            match Price::from_str(ts) {
                Ok(price) => price,
                Err(e) => {
                    return InstrumentParseResult::Failed {
                        symbol: symbol_str.to_string(),
                        instrument_type: "CurrencyPair".to_string(),
                        error: format!("Failed to parse tick_size '{}': {}", ts, e),
                    };
                }
            }
        }
        None => {
            // Fallback: use price_precision to calculate tick_size
            let precision = instrument.price_precision.unwrap_or(8);
            let tick_value = 10_f64.powi(-(precision as i32));
            Price::new(tick_value, precision)
        }
    };

    // Parse step size (minimum quantity increment)
    let step_size = match &instrument.min_quantity {
        Some(mq) => {
            match Quantity::from_str(mq) {
                Ok(quantity) => quantity,
                Err(e) => {
                    return InstrumentParseResult::Failed {
                        symbol: symbol_str.to_string(),
                        instrument_type: "CurrencyPair".to_string(),
                        error: format!("Failed to parse min_quantity '{}': {}", mq, e),
                    };
                }
            }
        }
        None => {
            // Fallback: use quantity_precision to calculate step_size
            let precision = instrument.quantity_precision.unwrap_or(8);
            let step_value = 10_f64.powi(-(precision as i32));
            Quantity::new(step_value, precision)
        }
    };

    // Parse max and min quantities
    let max_quantity = instrument.max_quantity.as_ref()
        .and_then(|mq| Quantity::from_str(mq).ok());
    let min_quantity = Some(step_size);

    // Parse max and min prices (if available)
    // Note: MEXC API may not provide these directly, so we leave them as None
    let max_price = None;
    let min_price = None;

    // Spot has no leverage, use 1.0 margin
    let default_margin = Decimal::new(1, 0);

    // Get precisions
    let price_precision = instrument.price_precision.unwrap_or_else(|| tick_size.precision);
    let size_precision = instrument.quantity_precision.unwrap_or_else(|| step_size.precision);

    let currency_pair = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_precision,
        size_precision,
        tick_size,
        step_size,
        None, // multiplier (not used for spot)
        Some(step_size), // lot_size
        max_quantity,
        min_quantity,
        None, // max_notional
        None, // min_notional (we have min_amount but it's in quote currency, not quantity)
        max_price,
        min_price,
        Some(default_margin),
        Some(default_margin),
        None, // maker_fee
        None, // taker_fee
        ts_init,
        ts_init,
    );

    InstrumentParseResult::Ok(Box::new(InstrumentAny::CurrencyPair(currency_pair)))
}


