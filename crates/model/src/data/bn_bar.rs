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

//! Binance kline bar data with venue-specific fields.

use std::{any::Any, fmt::Display, sync::Arc};

use nautilus_core::{UnixNanos, serialization::Serializable};
use serde::{Deserialize, Serialize};

use super::{Bar, BarType, CustomDataTrait, HasTsInit};
use crate::{
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

/// Represents a Binance kline bar with Binance-specific volume and trade fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct BnBar {
    /// The bar type for this bar.
    pub bar_type: BarType,
    /// The bars open price.
    pub open: Price,
    /// The bars high price.
    pub high: Price,
    /// The bars low price.
    pub low: Price,
    /// The bars close price.
    pub close: Price,
    /// The bars base asset volume.
    pub volume: Quantity,
    /// The quote asset volume.
    pub quote_volume: Quantity,
    /// The taker buy base asset volume.
    pub taker_buy_volume: Quantity,
    /// The taker buy quote asset volume.
    pub taker_buy_quote_volume: Quantity,
    /// The number of trades in this kline.
    pub trades_count: u64,
    /// The first trade ID in this kline.
    pub first_trade_id: i64,
    /// The last trade ID in this kline.
    pub last_trade_id: i64,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
}

impl BnBar {
    /// Creates a new [`BnBar`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        bar_type: BarType,
        open: Price,
        high: Price,
        low: Price,
        close: Price,
        volume: Quantity,
        quote_volume: Quantity,
        taker_buy_volume: Quantity,
        taker_buy_quote_volume: Quantity,
        trades_count: u64,
        first_trade_id: i64,
        last_trade_id: i64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            bar_type,
            open,
            high,
            low,
            close,
            volume,
            quote_volume,
            taker_buy_volume,
            taker_buy_quote_volume,
            trades_count,
            first_trade_id,
            last_trade_id,
            ts_event,
            ts_init,
        }
    }

    /// Creates a [`BnBar`] from a standard [`Bar`] and Binance-specific fields.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn from_bar(
        bar: Bar,
        quote_volume: Quantity,
        taker_buy_volume: Quantity,
        taker_buy_quote_volume: Quantity,
        trades_count: u64,
        first_trade_id: i64,
        last_trade_id: i64,
    ) -> Self {
        Self::new(
            bar.bar_type,
            bar.open,
            bar.high,
            bar.low,
            bar.close,
            bar.volume,
            quote_volume,
            taker_buy_volume,
            taker_buy_quote_volume,
            trades_count,
            first_trade_id,
            last_trade_id,
            bar.ts_event,
            bar.ts_init,
        )
    }

    /// Returns the instrument ID for this bar.
    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        self.bar_type.instrument_id()
    }

    /// Converts this value to a standard [`Bar`], dropping Binance-specific fields.
    #[must_use]
    pub fn as_bar(&self) -> Bar {
        Bar::new(
            self.bar_type,
            self.open,
            self.high,
            self.low,
            self.close,
            self.volume,
            self.ts_event,
            self.ts_init,
        )
    }
}

impl Display for BnBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            self.bar_type,
            self.open,
            self.high,
            self.low,
            self.close,
            self.volume,
            self.quote_volume,
            self.taker_buy_volume,
            self.taker_buy_quote_volume,
            self.trades_count,
            self.first_trade_id,
            self.last_trade_id,
            self.ts_event,
            self.ts_init,
        )
    }
}

impl Serializable for BnBar {}

impl HasTsInit for BnBar {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for BnBar {
    fn type_name(&self) -> &'static str {
        Self::type_name_static()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(*self)
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }

    fn type_name_static() -> &'static str {
        "BnBar"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let parsed: Self = serde_json::from_value(value)?;
        Ok(Arc::new(parsed))
    }
}

impl From<BnBar> for Bar {
    fn from(value: BnBar) -> Self {
        value.as_bar()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use rstest::rstest;

    use super::*;
    use crate::{
        data::BarSpecification,
        enums::{AggregationSource, BarAggregation, PriceType},
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };

    fn sample_bn_bar() -> BnBar {
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification {
                step: NonZeroUsize::new(1).unwrap(),
                aggregation: BarAggregation::Minute,
                price_type: PriceType::Last,
            },
            AggregationSource::External,
        );

        BnBar::new(
            bar_type,
            Price::from("100.0"),
            Price::from("110.0"),
            Price::from("90.0"),
            Price::from("105.0"),
            Quantity::from("12.5"),
            Quantity::from("1312.5"),
            Quantity::from("7.5"),
            Quantity::from("787.5"),
            42,
            1000,
            1041,
            UnixNanos::from(1_700_000_000_000_000_000),
            UnixNanos::from(1_700_000_000_100_000_000),
        )
    }

    #[rstest]
    fn test_as_bar_preserves_standard_fields() {
        let bn_bar = sample_bn_bar();
        let bar = bn_bar.as_bar();

        assert_eq!(bar.bar_type, bn_bar.bar_type);
        assert_eq!(bar.open, bn_bar.open);
        assert_eq!(bar.high, bn_bar.high);
        assert_eq!(bar.low, bn_bar.low);
        assert_eq!(bar.close, bn_bar.close);
        assert_eq!(bar.volume, bn_bar.volume);
        assert_eq!(bar.ts_event, bn_bar.ts_event);
        assert_eq!(bar.ts_init, bn_bar.ts_init);
    }

    #[rstest]
    fn test_custom_data_json_roundtrip() {
        let bn_bar = sample_bn_bar();
        let json = bn_bar.to_json().unwrap();
        let parsed = BnBar::from_json(serde_json::from_str(&json).unwrap()).unwrap();
        let parsed = parsed.as_any().downcast_ref::<BnBar>().unwrap();

        assert_eq!(parsed, &bn_bar);
    }
}
