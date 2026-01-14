// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Parsing utilities for MEXC WebSocket messages.
//!
//! This module provides functions to convert MEXC protobuf messages into Nautilus domain types.

use std::str::FromStr;

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarSpecification, BookOrder, Data, OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API,
        QuoteTick, TradeTick,
    },
    enums::{AggressorSide, BookAction, OrderSide, PriceType},
    identifiers::TradeId,
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};

// OrderId is a type alias for u64 in data::order
type OrderId = u64;
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::proto::{
    PublicAggreDealsV3Api, PublicAggreDealsV3ApiItem, PublicBookTickerV3Api,
    PublicDealsV3Api, PublicDealsV3ApiItem, PublicIncreaseDepthsV3Api,
    PublicLimitDepthsV3Api, PublicSpotKlineV3Api, PushDataV3ApiWrapper,
};

use super::error::{MexcWsError, MexcWsResult};

/// Parses a MEXC protobuf wrapper message into Nautilus data types.
///
/// # Errors
///
/// Returns an error if parsing fails or required fields are missing.
pub fn parse_protobuf_wrapper(
    wrapper: &PushDataV3ApiWrapper,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    ts_init: UnixNanos,
) -> MexcWsResult<Vec<Data>> {
    let symbol = wrapper
        .symbol
        .as_ref()
        .ok_or_else(|| MexcWsError::MissingField("symbol".to_string()))?;

    let instrument = instruments
        .get(&Ustr::from(symbol.as_str()))
        .ok_or_else(|| MexcWsError::InvalidSymbol(symbol.clone()))?;

    let body = wrapper
        .body
        .as_ref()
        .ok_or_else(|| MexcWsError::MissingField("body".to_string()))?;

    match body {
        crate::proto::push_data_v3_api_wrapper::Body::PublicDeals(msg) => {
            parse_public_deals(msg, instrument, ts_init)
        }
        crate::proto::push_data_v3_api_wrapper::Body::PublicAggreDeals(msg) => {
            parse_public_aggre_deals(msg, instrument, ts_init)
        }
        crate::proto::push_data_v3_api_wrapper::Body::PublicIncreaseDepths(msg) => {
            parse_public_increase_depths(msg, instrument, ts_init)
        }
        crate::proto::push_data_v3_api_wrapper::Body::PublicLimitDepths(msg) => {
            parse_public_limit_depths(msg, instrument, ts_init)
        }
        crate::proto::push_data_v3_api_wrapper::Body::PublicBookTicker(msg) => {
            parse_public_book_ticker(msg, instrument, ts_init)
        }
        crate::proto::push_data_v3_api_wrapper::Body::PublicSpotKline(msg) => {
            parse_public_spot_kline(msg, instrument, ts_init)
        }
        _ => {
            // Other message types not yet implemented
            Ok(vec![])
        }
    }
}

/// Parses public deals (trades) messages.
fn parse_public_deals(
    msg: &PublicDealsV3Api,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> MexcWsResult<Vec<Data>> {
    let mut trades = Vec::new();

    for deal in &msg.deals {
        match parse_deal_item(deal, instrument, ts_init) {
            Ok(trade) => trades.push(Data::Trade(trade)),
            Err(e) => {
                log::warn!("Failed to parse deal item: {e}");
            }
        }
    }

    Ok(trades)
}

/// Parses a single deal item into a TradeTick.
fn parse_deal_item(
    deal: &PublicDealsV3ApiItem,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> MexcWsResult<TradeTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price_decimal = Decimal::from_str(&deal.price)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to parse price: {e}")))?;
    let quantity_decimal = Decimal::from_str(&deal.quantity)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to parse quantity: {e}")))?;

    // MEXC trade_type: 1 = buy, 2 = sell
    let aggressor_side = match deal.trade_type {
        1 => AggressorSide::Buyer,
        2 => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    };

    // Generate trade ID from timestamp and price if not available
    let trade_id = TradeId::new(&format!("{}_{}", deal.time, deal.price));

    // Convert milliseconds to nanoseconds
    let ts_event = UnixNanos::from(deal.time as u64 * 1_000_000);

    let price = Price::from_decimal_dp(price_decimal, price_precision)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to create Price: {e}")))?;
    let quantity = Quantity::from_decimal_dp(quantity_decimal, size_precision)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to create Quantity: {e}")))?;

    TradeTick::new_checked(
        instrument_id,
        price,
        quantity,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
    .map_err(|e| MexcWsError::ParseError(format!("Failed to create TradeTick: {e}")))
}

/// Parses public aggregate deals messages.
fn parse_public_aggre_deals(
    msg: &PublicAggreDealsV3Api,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> MexcWsResult<Vec<Data>> {
    let mut trades = Vec::new();

    for deal in &msg.deals {
        match parse_aggre_deal_item(deal, instrument, ts_init) {
            Ok(trade) => trades.push(Data::Trade(trade)),
            Err(e) => {
                log::warn!("Failed to parse aggregate deal item: {e}");
            }
        }
    }

    Ok(trades)
}

/// Parses a single aggregate deal item.
fn parse_aggre_deal_item(
    deal: &PublicAggreDealsV3ApiItem,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> MexcWsResult<TradeTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price_decimal = Decimal::from_str(&deal.price)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to parse price: {e}")))?;
    let quantity_decimal = Decimal::from_str(&deal.quantity)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to parse quantity: {e}")))?;

    // MEXC trade_type: 1 = buy, 2 = sell
    let aggressor_side = match deal.trade_type {
        1 => AggressorSide::Buyer,
        2 => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    };

    let trade_id = TradeId::new(&format!("{}_{}", deal.time, deal.price));
    let ts_event = UnixNanos::from(deal.time as u64 * 1_000_000);

    let price = Price::from_decimal_dp(price_decimal, price_precision)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to create Price: {e}")))?;
    let quantity = Quantity::from_decimal_dp(quantity_decimal, size_precision)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to create Quantity: {e}")))?;

    TradeTick::new_checked(
        instrument_id,
        price,
        quantity,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
    .map_err(|e| MexcWsError::ParseError(format!("Failed to create TradeTick: {e}")))
}

/// Parses public increase depths (incremental order book updates).
fn parse_public_increase_depths(
    msg: &PublicIncreaseDepthsV3Api,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> MexcWsResult<Vec<Data>> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let mut deltas_vec = Vec::new();
    let ts_event = ts_init;
    let flags = 0u8;
    let mut sequence = 0u64;

    // Parse bid side
    for bid in &msg.bids {
        let price_decimal = rust_decimal::Decimal::from_str(&bid.price)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to parse bid price: {e}")))?;
        let size_decimal = rust_decimal::Decimal::from_str(&bid.quantity)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to parse bid quantity: {e}")))?;

        let price = Price::from_decimal_dp(price_decimal, price_precision)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to create bid Price: {e}")))?;
        let size = Quantity::from_decimal_dp(size_decimal, size_precision)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to create bid Quantity: {e}")))?;

        let order = BookOrder::new(OrderSide::Buy, price, size, OrderId::from(sequence));
        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Update,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        deltas_vec.push(delta);
        sequence += 1;
    }

    // Parse ask side
    for ask in &msg.asks {
        let price_decimal = Decimal::from_str(&ask.price)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to parse ask price: {e}")))?;
        let size_decimal = Decimal::from_str(&ask.quantity)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to parse ask quantity: {e}")))?;

        let price = Price::from_decimal_dp(price_decimal, price_precision)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to create ask Price: {e}")))?;
        let size = Quantity::from_decimal_dp(size_decimal, size_precision)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to create ask Quantity: {e}")))?;

        let order = BookOrder::new(OrderSide::Sell, price, size, OrderId::from(sequence));
        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Update,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        deltas_vec.push(delta);
        sequence += 1;
    }

    let deltas = OrderBookDeltas::new(instrument_id, deltas_vec);
    let deltas_api = OrderBookDeltas_API::new(deltas);

    Ok(vec![Data::Deltas(deltas_api)])
}

/// Parses public limit depths (full order book snapshot).
fn parse_public_limit_depths(
    msg: &PublicLimitDepthsV3Api,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> MexcWsResult<Vec<Data>> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let mut deltas_vec = Vec::new();
    let ts_event = ts_init;
    let flags = 0u8;
    let mut sequence = 0u64;

    // Parse bid side
    for bid in &msg.bids {
        let price_decimal = rust_decimal::Decimal::from_str(&bid.price)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to parse bid price: {e}")))?;
        let size_decimal = rust_decimal::Decimal::from_str(&bid.quantity)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to parse bid quantity: {e}")))?;

        let price = Price::from_decimal_dp(price_decimal, price_precision)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to create bid Price: {e}")))?;
        let size = Quantity::from_decimal_dp(size_decimal, size_precision)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to create bid Quantity: {e}")))?;

        let order = BookOrder::new(OrderSide::Buy, price, size, OrderId::from(sequence));
        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        deltas_vec.push(delta);
        sequence += 1;
    }

    // Parse ask side
    for ask in &msg.asks {
        let price_decimal = Decimal::from_str(&ask.price)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to parse ask price: {e}")))?;
        let size_decimal = Decimal::from_str(&ask.quantity)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to parse ask quantity: {e}")))?;

        let price = Price::from_decimal_dp(price_decimal, price_precision)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to create ask Price: {e}")))?;
        let size = Quantity::from_decimal_dp(size_decimal, size_precision)
            .map_err(|e| MexcWsError::ParseError(format!("Failed to create ask Quantity: {e}")))?;

        let order = BookOrder::new(OrderSide::Sell, price, size, OrderId::from(sequence));
        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        );
        deltas_vec.push(delta);
        sequence += 1;
    }

    let deltas = OrderBookDeltas::new(instrument_id, deltas_vec);
    let deltas_api = OrderBookDeltas_API::new(deltas);

    Ok(vec![Data::Deltas(deltas_api)])
}

/// Parses public book ticker (best bid/ask) messages.
fn parse_public_book_ticker(
    msg: &PublicBookTickerV3Api,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> MexcWsResult<Vec<Data>> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let bid_price_decimal = Decimal::from_str(&msg.bid_price)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to parse bid price: {e}")))?;
    let bid_size_decimal = Decimal::from_str(&msg.bid_quantity)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to parse bid quantity: {e}")))?;

    let ask_price_decimal = Decimal::from_str(&msg.ask_price)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to parse ask price: {e}")))?;
    let ask_size_decimal = Decimal::from_str(&msg.ask_quantity)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to parse ask quantity: {e}")))?;

    let bid_price = Price::from_decimal_dp(bid_price_decimal, price_precision)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to create bid Price: {e}")))?;
    let bid_size = Quantity::from_decimal_dp(bid_size_decimal, size_precision)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to create bid Quantity: {e}")))?;

    let ask_price = Price::from_decimal_dp(ask_price_decimal, price_precision)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to create ask Price: {e}")))?;
    let ask_size = Quantity::from_decimal_dp(ask_size_decimal, size_precision)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to create ask Quantity: {e}")))?;

    let quote = QuoteTick::new(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_init,
        ts_init,
    );

    Ok(vec![Data::Quote(quote)])
}

/// Parses public spot kline (candlestick) messages.
fn parse_public_spot_kline(
    msg: &PublicSpotKlineV3Api,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> MexcWsResult<Vec<Data>> {
    use nautilus_model::{
        data::{BarSpecification, BarType},
        enums::{AggregationSource, PriceType},
    };

    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    // Parse interval string (e.g., "Min1", "Min5", "Hour1", "Day1")
    let spec = parse_kline_interval(&msg.interval)?;
    let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

    let open_decimal = Decimal::from_str(&msg.opening_price)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to parse open price: {e}")))?;
    let high_decimal = Decimal::from_str(&msg.highest_price)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to parse high price: {e}")))?;
    let low_decimal = Decimal::from_str(&msg.lowest_price)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to parse low price: {e}")))?;
    let close_decimal = Decimal::from_str(&msg.closing_price)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to parse close price: {e}")))?;
    let volume_decimal = Decimal::from_str(&msg.volume)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to parse volume: {e}")))?;

    let open = Price::from_decimal_dp(open_decimal, price_precision)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to create open Price: {e}")))?;
    let high = Price::from_decimal_dp(high_decimal, price_precision)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to create high Price: {e}")))?;
    let low = Price::from_decimal_dp(low_decimal, price_precision)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to create low Price: {e}")))?;
    let close = Price::from_decimal_dp(close_decimal, price_precision)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to create close Price: {e}")))?;
    let volume = Quantity::from_decimal_dp(volume_decimal, size_precision)
        .map_err(|e| MexcWsError::ParseError(format!("Failed to create volume Quantity: {e}")))?;

    // Convert seconds to nanoseconds
    let ts_event = UnixNanos::from(msg.window_start as u64 * 1_000_000_000);

    let bar = Bar::new(bar_type, open, high, low, close, volume, ts_event, ts_init);

    Ok(vec![Data::Bar(bar)])
}

/// Parses MEXC kline interval string to BarSpecification.
fn parse_kline_interval(interval: &str) -> MexcWsResult<BarSpecification> {
    use nautilus_model::{
        data::BarSpecification,
        enums::{BarAggregation, PriceType},
    };

    let spec = match interval {
        "Min1" => BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
        "Min5" => BarSpecification::new(5, BarAggregation::Minute, PriceType::Last),
        "Min15" => BarSpecification::new(15, BarAggregation::Minute, PriceType::Last),
        "Min30" => BarSpecification::new(30, BarAggregation::Minute, PriceType::Last),
        "Min60" => BarSpecification::new(60, BarAggregation::Minute, PriceType::Last),
        "Hour4" => BarSpecification::new(4, BarAggregation::Hour, PriceType::Last),
        "Hour8" => BarSpecification::new(8, BarAggregation::Hour, PriceType::Last),
        "Day1" => BarSpecification::new(1, BarAggregation::Day, PriceType::Last),
        "Week1" => BarSpecification::new(1, BarAggregation::Week, PriceType::Last),
        "Month1" => BarSpecification::new(1, BarAggregation::Month, PriceType::Last),
        _ => {
            return Err(MexcWsError::ParseError(format!(
                "Unsupported kline interval: {interval}"
            )));
        }
    };

    Ok(spec)
}
