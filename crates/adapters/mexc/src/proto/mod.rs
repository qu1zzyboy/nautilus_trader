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

//! Protocol Buffer definitions for MEXC.
//!
//! This module provides protobuf message definitions for MEXC WebSocket communication.
//! MEXC uses protobuf for efficient binary message encoding.
//!
//! The protobuf code is generated from .proto files in `src/websocket-proto/`.
//! To regenerate, run: `./generate_proto.sh` which will automatically build and copy
//! the generated file to `src/proto/mexc_proto.rs`

// Include the manually generated protobuf code
mod mexc_proto;
pub use mexc_proto::*;

// Re-export commonly used nested types for convenience
pub use push_data_v3_api_wrapper::Body;

/// Main MEXC protobuf message wrapper.
///
/// This is the top-level message type that wraps all MEXC WebSocket messages.
pub type MexcProtoMessage = PushDataV3ApiWrapper;

