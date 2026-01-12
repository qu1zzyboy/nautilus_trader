# MEXC Adapter

MEXC exchange integration adapter for the Nautilus trading engine.

## Overview

This adapter provides integration with the MEXC cryptocurrency exchange. MEXC uses **Protocol Buffers (protobuf)** for WebSocket communication, requiring binary message handling.

## Project Structure

```
crates/adapters/mexc/
├── Cargo.toml              # Dependencies including prost for protobuf
├── README.md               # This file
└── src/
    ├── lib.rs              # Main module exports
    ├── config.rs           # Configuration types
    ├── error.rs            # Error types
    ├── common/             # Shared utilities
    │   ├── mod.rs
    │   ├── consts.rs       # Constants (URLs, venue ID, etc.)
    │   ├── enums.rs        # Enumerations (operations, topics)
    │   └── parse.rs        # Parsing utilities
    ├── proto/              # Protobuf definitions
    │   └── mod.rs          # Protobuf message types (TODO: implement)
    ├── websocket/          # WebSocket client
    │   ├── mod.rs
    │   ├── client.rs       # WebSocket client implementation
    │   ├── handler.rs      # Message handler (processes binary protobuf)
    │   ├── messages.rs     # Internal message types
    │   ├── error.rs        # WebSocket errors
    │   ├── enums.rs        # WebSocket enumerations
    │   └── parse.rs        # Message parsing utilities
    ├── http/               # HTTP client (REST API)
    │   ├── mod.rs
    │   ├── client.rs       # HTTP client (TODO: implement)
    │   └── error.rs        # HTTP errors
    └── data/               # Data client
        └── mod.rs          # DataClient implementation (TODO: implement)
```

## Key Features

- **Binary Protobuf Support**: Handler processes `Message::Binary` from WebSocket
- **WebSocket Client**: Full WebSocket client with reconnection support
- **Subscription Management**: Topic-based subscription state tracking
- **Instrument Caching**: Local cache for instrument definitions

## Next Steps

### 1. Implement Protobuf Message Definitions

The `proto/mod.rs` file currently has placeholder structures. You need to:

- Obtain MEXC's `.proto` files or reverse engineer the message format
- Define the actual protobuf message structures
- Implement conversion from protobuf to Nautilus domain models

Example approach:
```rust
// Option 1: Use .proto files with prost-build
// Create src/proto/mexc.proto and use build.rs to generate code

// Option 2: Manually define using prost::Message derive
#[derive(Clone, PartialEq, Message)]
pub struct MexcProtoMessage {
    #[prost(oneof="mexc_proto_message::Message", tags="1,2,3")]
    pub message: Option<mexc_proto_message::Message>,
}
```

### 2. Complete Handler Implementation

In `websocket/handler.rs`:

- Implement `parse_protobuf_message()` to decode binary messages
- Implement conversion from protobuf messages to `NautilusWsMessage`
- Add message type handlers (quotes, trades, order book, etc.)

### 3. Implement Subscription Logic

In `websocket/client.rs` and `websocket/handler.rs`:

- Build protobuf subscribe/unsubscribe messages
- Handle subscription confirmations
- Implement reconnection resubscription logic

### 4. Implement Data Parsing

In `websocket/parse.rs`:

- Convert protobuf market data to Nautilus `Data` types
- Handle different message types (depth, trade, kline, ticker)
- Parse instrument information

### 5. Implement HTTP Client

In `http/client.rs`:

- Implement REST API calls
- Handle authentication
- Request instrument definitions
- Request historical data

### 6. Implement Data Client

In `data/mod.rs`:

- Implement `DataClient` trait
- Handle subscription commands from DataEngine
- Forward WebSocket messages to DataEngine

## Testing

Once implemented, you can test the adapter:

```rust
// Example test structure
#[tokio::test]
async fn test_websocket_connection() {
    let client = MexcWebSocketClient::new(
        None,  // Use default URL
        None,  // No auth for public data
        None,
        None,
        Some(30),  // 30 second heartbeat
    ).unwrap();
    
    // Test connection
    // ...
}
```

## References

- [MEXC API Documentation](https://mexcdevelop.github.io/apidocs/)
- [Protocol Buffers Guide](https://protobuf.dev/)
- [prost Documentation](https://docs.rs/prost/)

## Notes

- MEXC uses binary protobuf messages, not JSON
- Handler must process `Message::Binary` from WebSocket
- Use `prost::Message::decode()` to parse binary data
- Use `prost::Message::encode()` to create binary messages for sending

