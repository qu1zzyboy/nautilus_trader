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

//! Integration tests for MEXC adapter factories.

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clock::TestClock,
    live::runner::{set_data_event_sender, set_exec_event_sender},
    messages::{DataEvent, ExecutionEvent},
};
use nautilus_model::identifiers::{AccountId, ClientId, TraderId};
use nautilus_system::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use rstest::rstest;

use nautilus_mexc::{
    config::{MexcDataClientConfig, MexcExecClientConfig},
    factories::{MexcDataClientFactory, MexcExecutionClientFactory},
};

fn setup_test_env() {
    // Initialize data event sender for tests
    let (data_sender, _data_receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(data_sender);
    
    // Initialize execution event sender for tests
    let (exec_sender, _exec_receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(exec_sender);
}

#[rstest]
fn test_mexc_data_client_factory_creation() {
    setup_test_env();
    let factory = MexcDataClientFactory::new();
    assert_eq!(factory.name(), "MEXC");
    assert_eq!(factory.config_type(), "MexcDataClientConfig");
}

#[rstest]
fn test_mexc_data_client_factory_default() {
    setup_test_env();
    let factory = MexcDataClientFactory::new();
    assert_eq!(factory.name(), "MEXC");
}

#[rstest]
fn test_mexc_data_client_config_implements_client_config() {
    setup_test_env();
    let config = MexcDataClientConfig::default();

    let boxed_config: Box<dyn ClientConfig> = Box::new(config);
    let downcasted = boxed_config
        .as_any()
        .downcast_ref::<MexcDataClientConfig>();

    assert!(downcasted.is_some());
}

#[rstest]
fn test_mexc_data_client_factory_creates_client() {
    setup_test_env();
    let factory = MexcDataClientFactory::new();
    let config = MexcDataClientConfig::default();
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));

    let result = factory.create("MEXC-TEST", &config, cache, clock);
    assert!(result.is_ok());

    let client = result.unwrap();
    assert_eq!(client.client_id(), ClientId::from("MEXC-TEST"));
    assert!(client.venue().is_some());
}

#[rstest]
fn test_mexc_execution_client_factory_creation() {
    setup_test_env();
    let factory = MexcExecutionClientFactory::new();
    assert_eq!(factory.name(), "MEXC");
    assert_eq!(factory.config_type(), "MexcExecClientConfig");
}

#[rstest]
fn test_mexc_execution_client_factory_default() {
    setup_test_env();
    let factory = MexcExecutionClientFactory::new();
    assert_eq!(factory.name(), "MEXC");
}

#[rstest]
fn test_mexc_exec_client_config_implements_client_config() {
    setup_test_env();
    let config = MexcExecClientConfig::default();

    let boxed_config: Box<dyn ClientConfig> = Box::new(config);
    let downcasted = boxed_config
        .as_any()
        .downcast_ref::<MexcExecClientConfig>();

    assert!(downcasted.is_some());
}

#[rstest]
fn test_mexc_execution_client_factory_creates_client() {
    setup_test_env();
    let factory = MexcExecutionClientFactory::new();
    let config = MexcExecClientConfig {
        trader_id: TraderId::from("TESTER-001"),
        account_id: AccountId::from("MEXC-001"),
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
        ..Default::default()
    };
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));

    let result = factory.create("MEXC-EXEC-TEST", &config, cache, clock);
    assert!(result.is_ok());

    let client = result.unwrap();
    assert_eq!(client.client_id(), ClientId::from("MEXC-EXEC-TEST"));
    assert_eq!(client.account_id(), AccountId::from("MEXC-001"));
}

