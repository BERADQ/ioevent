# `IOEVENT`

[![GitHub License](https://img.shields.io/github/license/BERADQ/ioevent)](https://github.com/BERADQ/ioevent/blob/main/LICENSE)
![GitHub Issues or Pull Requests](https://img.shields.io/github/issues/BERADQ/ioevent)
![Crates.io Version](https://img.shields.io/crates/v/ioevent)
[![docs.rs](https://img.shields.io/docsrs/ioevent)](https://docs.rs/ioevent/latest/ioevent)

A lightweight Rust crate for building event-driven applications on top of Tokio's async I/O streams with low overhead. Facilitates decoupled architectures, suitable for inter-process communication or modular designs.

## Features
- **Event-driven Architecture**: Transforms async I/O operations into unified event streams
- **Tokio Integration**: Built upon and integrates seamlessly with Tokio's async runtime
- **Extensibility**: Supports custom event types and dynamic handler registration
- **Bi-directional Communication**: Enables event emission and response handling through Procedure Calls

## Quick Examples

See full working examples in the [`examples`](https://github.com/BERADQ/ioevent/blob/main/examples) directory.

### Define Events

```rust
#[derive(Deserialize, Serialize, Debug, Event)]
pub struct Ping {
    pub timestamp: i64,
}

// Custom event tag
#[derive(Deserialize, Serialize, Debug, Event)]
#[event(tag = "com::demo::my::Event")]
pub struct CustomEvent(pub String, pub i64);
```

### Create Subscribers

```rust
#[subscriber]
async fn handle_ping(state: State<AppState>, event: Ping) -> Result {
    state.wright.emit(&Pong { timestamp: event.timestamp })?;
    Ok(())
}

static SUBSCRIBERS: &[Subscriber<AppState>] = &[create_subscriber!(handle_ping)];
```

### Build and Run

```rust
// Build the event bus
let subscribers = Subscribers::init(SUBSCRIBERS);
let mut builder = BusBuilder::new(subscribers);
builder.add_pair(IoPair::stdio());
let (bus, effect_wright) = builder.build();

// Run the bus
let state = State::new(AppState {}, effect_wright);
bus.run(state, &|error| { eprintln!("{:?}", error); }).await;
```

## Procedure Call (RPC)

See the complete RPC example in [`examples/rpc`](https://github.com/BERADQ/ioevent/blob/main/examples/rpc).

### Define Procedures

```rust
// Custom the path
#[derive(Deserialize, Serialize, Debug, ProcedureCall)]
#[procedure(path = "com::demo::my::CallPrint")]
pub struct CallPrint(pub String);
impl ProcedureCallRequest for CallPrint {
    type RESPONSE = CallPrintResponse;
}

#[derive(Deserialize, Serialize, Debug, ProcedureCall)]
pub struct CallPrintResponse(pub u64);
impl ProcedureCallResponse for CallPrintResponse {}
```

### Handle Procedures

```rust
#[procedure]
async fn print_handler(request: CallPrint) -> Result {
    println!("Message: {}", request.0);
    Ok(CallPrintResponse(42))
}
```

### Make Calls

```rust
let response = state.call(&CallPrint("Hello".to_string())).await?;
//  ^ CallPrintResponse
```

## License

This project is licensed under the Unlicense - see the [LICENSE](https://github.com/BERADQ/ioevent/blob/main/LICENSE) file for details.