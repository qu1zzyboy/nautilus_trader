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

use crate::{harmony::net_taker_imbalance::NetTakerImbalance, indicator::Indicator};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl NetTakerImbalance {
    /// Creates a new `NetTakerImbalance` instance.
    #[new]
    #[pyo3(signature = (epsilon=None))]
    fn py_new(epsilon: Option<f64>) -> Self {
        Self::new(epsilon)
    }

    fn __repr__(&self) -> String {
        format!("NetTakerImbalance({})", self.epsilon)
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "epsilon")]
    const fn py_epsilon(&self) -> f64 {
        self.epsilon
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
    fn py_update_raw(&mut self, quote_volume: f64, taker_buy_quote_volume: f64) {
        self.update_raw(quote_volume, taker_buy_quote_volume);
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}
