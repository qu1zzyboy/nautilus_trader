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

//! Live market data client implementation for the MEXC adapter.

use std::sync::{Arc, RwLock};

use ahash::AHashMap;
use nautilus_core::MUTEX_POISONED;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};

mod client;

pub use client::MexcDataClient;

/// Formats a MEXC stream name for the given instrument and channel.
///
/// MEXC stream format for protobuf channels: "channel.pb@interval@symbol"
/// Examples:
/// - "spot@public.aggre.bookTicker.v3.api.pb@100ms@BTCUSDT"
/// - "spot@public.aggre.depth.v3.api.pb@100ms@BTCUSDT"
/// - "spot@public.aggre.deals.v3.api.pb@100ms@BTCUSDT"
/// - "spot@public.bookTicker.batch.v3.api.pb@BTCUSDT"
fn format_mexc_stream(channel: &str, symbol: &str) -> String {
    // For protobuf channels, add .pb suffix and interval if needed
    if channel.contains("bookTicker") {
        // Use batch version for bookTicker (no interval needed)
        if channel.contains("batch") {
            format!("{}.pb@{}", channel, symbol)
        } else {
            // Use aggre version with interval
            format!("{}.pb@100ms@{}", channel, symbol)
        }
    } else if channel.contains("aggre") {
        // Aggregated channels need interval
        format!("{}.pb@100ms@{}", channel, symbol)
    } else {
        // Fallback: simple format
        format!("{}@{}", channel, symbol)
    }
}

/// Formats a symbol from InstrumentId for MEXC API.
///
/// MEXC uses uppercase symbols without underscore (e.g., "BTCUSDT").
fn format_mexc_symbol(instrument_id: &InstrumentId) -> String {
    instrument_id.symbol.as_str().to_uppercase().replace('_', "")
}

/// Converts a bar interval to MEXC kline interval string.
///
/// MEXC supports: Min1, Min5, Min15, Min30, Hour1, Hour4, Day1, Week1, Month1
fn bar_interval_to_mexc_interval(spec: &nautilus_model::data::BarSpecification) -> anyhow::Result<String> {
    use nautilus_model::enums::{BarAggregation, PriceType};
    
    // MEXC requires Last price type for klines
    if spec.price_type != PriceType::Last {
        anyhow::bail!("MEXC klines only support Last price type, got: {:?}", spec.price_type);
    }

    let interval = match (spec.step.get(), spec.aggregation) {
        (1, BarAggregation::Minute) => "Min1",
        (5, BarAggregation::Minute) => "Min5",
        (15, BarAggregation::Minute) => "Min15",
        (30, BarAggregation::Minute) => "Min30",
        (1, BarAggregation::Hour) => "Hour1",
        (4, BarAggregation::Hour) => "Hour4",
        (1, BarAggregation::Day) => "Day1",
        (1, BarAggregation::Week) => "Week1",
        (1, BarAggregation::Month) => "Month1",
        _ => anyhow::bail!("Unsupported bar interval: step={}, aggregation={:?}", spec.step.get(), spec.aggregation),
    };

    Ok(interval.to_string())
}

fn upsert_instrument(
    cache: &Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    instrument: InstrumentAny,
) {
    let mut guard = cache.write().expect(MUTEX_POISONED);
    guard.insert(instrument.id(), instrument);
}
