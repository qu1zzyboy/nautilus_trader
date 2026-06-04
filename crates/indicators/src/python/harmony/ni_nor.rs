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

use pyo3::prelude::*;

use crate::{harmony::ni_nor::NiNor, indicator::Indicator};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl NiNor {
    /// Creates a new `NiNor` instance.
    #[new]
    #[pyo3(signature = (ewm_alpha=0.1, normalization_period=1400))]
    fn py_new(ewm_alpha: Option<f64>, normalization_period: Option<usize>) -> Self {
        Self::new(ewm_alpha, normalization_period)
    }

    fn __repr__(&self) -> String {
        format!("NiNor({}, {})", self.ewm_alpha, self.normalization_period)
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "ewm_alpha")]
    const fn py_ewm_alpha(&self) -> f64 {
        self.ewm_alpha
    }

    #[getter]
    #[pyo3(name = "normalization_period")]
    const fn py_normalization_period(&self) -> usize {
        self.normalization_period
    }

    #[getter]
    #[pyo3(name = "normalization_epsilon")]
    const fn py_normalization_epsilon(&self) -> f64 {
        self.normalization_epsilon
    }

    #[getter]
    #[pyo3(name = "raw_value")]
    const fn py_raw_value(&self) -> f64 {
        self.raw_value
    }

    #[getter]
    #[pyo3(name = "smoothed_value")]
    const fn py_smoothed_value(&self) -> f64 {
        self.smoothed_value
    }

    #[getter]
    #[pyo3(name = "value")]
    const fn py_value(&self) -> f64 {
        self.value
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "initialized")]
    const fn py_initialized(&self) -> bool {
        self.initialized
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, close: f64, quote_volume: f64, taker_buy_quote_volume: f64) {
        self.update_raw(close, quote_volume, taker_buy_quote_volume);
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}
