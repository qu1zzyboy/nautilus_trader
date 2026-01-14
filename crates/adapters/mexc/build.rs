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

//! Build script for compiling MEXC protobuf definitions.

use std::{env, path::PathBuf};

fn main() {
    // Skip protobuf compilation when building docs (docs.rs doesn't have protoc)
    if env::var("DOCS_RS").is_ok() {
        println!("cargo:rustc-cfg=docs_rs");
        return;
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = PathBuf::from(&manifest_dir);
    let proto_dir = manifest_path.join("src/websocket-proto");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));

    // Tell Cargo to rerun this build script if any proto files change
    println!("cargo:rerun-if-changed={}", proto_dir.display());

    // Check if proto directory exists
    if !proto_dir.exists() {
        println!("cargo:warning=Proto directory not found: {}, skipping protobuf compilation", proto_dir.display());
        println!("cargo:warning=Using pre-generated proto files from src/proto/mexc_proto.rs");
        return;
    }

    // Collect all .proto files
    let proto_files: Vec<PathBuf> = match std::fs::read_dir(&proto_dir) {
        Ok(dir) => dir
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension()?.to_str()? == "proto" {
                    Some(path)
                } else {
                    None
                }
            })
            .collect(),
        Err(e) => {
            println!("cargo:warning=Failed to read proto directory: {e}, skipping protobuf compilation");
            println!("cargo:warning=Using pre-generated proto files from src/proto/mexc_proto.rs");
            return;
        }
    };

    if proto_files.is_empty() {
        println!("cargo:warning=No protobuf files found in {}", proto_dir.display());
        return;
    }

    // Configure prost-build
    let mut config = prost_build::Config::new();
    config.out_dir(&out_dir);
    
    // Enable protobuf well-known types if needed
    config.bytes(["."]);
    
    // Note: prost-build already generates #[derive(Clone, PartialEq, Message)]
    // Don't add duplicate derives to avoid conflicts
    
    // Compile all proto files
    // Note: We compile PushDataV3ApiWrapper.proto which imports all others
    let main_proto = proto_dir.join("PushDataV3ApiWrapper.proto");
    if main_proto.exists() {
        config
            .compile_protos(&[main_proto], &[proto_dir])
            .expect("Failed to compile protobuf files");
    } else {
        // Fallback: compile all proto files
        config
            .compile_protos(&proto_files, &[proto_dir])
            .expect("Failed to compile protobuf files");
    }
    
    // List generated files for debugging
    if let Ok(entries) = std::fs::read_dir(&out_dir) {
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|s| s.to_str()) == Some("rs") {
                println!("cargo:warning=Generated file: {:?}", entry.path());
            }
        }
    }

    println!("Protobuf files compiled to: {}", out_dir.display());
}

