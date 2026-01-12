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

//! Live market data client implementation for the MEXC adapter.

// TODO: Implement DataClient
// This is a placeholder module structure

use async_trait::async_trait;
use nautilus_data::client::DataClient;
use nautilus_model::identifiers::ClientId;

/// MEXC data client implementation.
#[derive(Debug)]
pub struct MexcDataClient {
    client_id: ClientId,
}

impl MexcDataClient {
    /// Creates a new [`MexcDataClient`] instance.
    pub fn new(client_id: ClientId) -> Self {
        Self { client_id }
    }
}

#[async_trait(?Send)]
impl DataClient for MexcDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<nautilus_model::identifiers::Venue> {
        Some(*crate::common::consts::MEXC_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        // TODO: Implement
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        // TODO: Implement
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        // TODO: Implement
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        // TODO: Implement
        Ok(())
    }

    fn is_connected(&self) -> bool {
        // TODO: Implement
        false
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }
}

