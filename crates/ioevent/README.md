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

## Quick

**1. Define Events**

Events are simple structs or enums implementing `serde::Serialize`, `serde::Deserialize`, and deriving `ioevent::Event`.

```rust
use serde::{Deserialize, Serialize};
use ioevent::prelude::*;

#[derive(Deserialize, Serialize, Debug, Event)]
pub struct Ping {
    pub timestamp: i64,
}

#[derive(Deserialize, Serialize, Debug, Event)]
pub struct Pong {
    pub timestamp: i64,
}
```

**2. Create Subscribers**

Subscribers are async functions that handle specific event types. Use the `#[subscriber]` attribute and the `create_subscriber!` macro.

```rust
use ioevent::prelude::*;

#[derive(Clone)]
struct AppState {
    // Your application state
}

#[subscriber]
async fn handle_ping(state: State<AppState>, event: Ping) -> Result {
    let pong = Pong { timestamp: event.timestamp };
    state.wright.emit(&pong)?;
    Ok(())
}

#[subscriber]
async fn log_all_event(event: EventData) {
    eprintln!("Event received: {:?}", event);
}

static SUBSCRIBERS: &[Subscriber<AppState>] = &[
    create_subscriber!(handle_ping),
    create_subscriber!(log_all_event),
];
```

**3. Build the Event Bus**

Use `BusBuilder` to configure and create the event bus.

```rust
use ioevent::prelude::*;

fn setup_bus() -> (Bus<AppState>, EffectWright) {
    let subscribers = Subscribers::init(SUBSCRIBERS);
    let mut builder = BusBuilder::new(subscribers);
    
    // Add I/O pairs
    builder.add_pair(IoPair::stdio());
    
    builder.build()
}
```

**4. Run the Bus**

Start the event processing loop using `bus.run()`.

```rust
use ioevent::prelude::*;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let (bus, effect_wright) = setup_bus();
    let state = State::new(AppState {}, effect_wright.clone());
    
    // Error handler
    let error_handler = |error: BusError| {
        eprintln!("Bus Error: {:?}", error);
    };
    
    // Run the bus
    let bus_handle = bus.run(state, &error_handler);
    
    // Example: Send periodic events
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let event = Ping { timestamp: 0 };
            if let Err(e) = effect_wright.emit(&event) {
                eprintln!("Failed to send event: {:?}", e);
            }
        }
    });
    
    bus_handle.await;
}
```

## Procedure Call (RPC)

`ioevent` supports request/response patterns via Procedure Calls. Here's how to implement RPC:

1. Define the procedure call types:

```rust
use ioevent::rpc::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, ProcedureCall)]
// By optional: custom the path
#[procedure(path = "com::demo::my::CallPrint")]
pub struct CallPrint(pub String);
impl ProcedureCallRequest for CallPrint {
    type RESPONSE = CallPrintResponse;
}

#[derive(Deserialize, Serialize, Debug, ProcedureCall)]
pub struct CallPrintResponse(pub u64);
impl ProcedureCallResponse for CallPrintResponse {}
```

2. Implement the procedure handler:

```rust
use ioevent::prelude::*;

#[derive(Clone, Default)]
struct MyState {
    pdc_wright: DefaultProcedureWright,
}

impl ProcedureCallWright for MyState {
    async fn next_echo(&self) -> u64 {
        self.pdc_wright.next_echo().await
    }
}

#[procedure]
async fn print_test(e: CallPrint) -> Result {
    println!("Received message: {}", e.0);
    Ok(CallPrintResponse(42))
}
```

3. Make RPC calls from the client:

```rust
use ioevent::prelude::*;

#[derive(Clone, Default)]
struct MyState {
    pdc_wright: DefaultProcedureWright,
    counter: Arc<AtomicU64>,
}

impl ProcedureCallWright for MyState {
    async fn next_echo(&self) -> u64 {
        self.pdc_wright.next_echo().await
    }
}

#[subscriber]
async fn call_print(state: State<MyState>, _e: EmptyEvent) -> Result {
    let call = state
        .call(&CallPrint(format!("Counter: {}", state.counter.load(Ordering::Relaxed))))
        .await;

    if let Ok(response) = call {
        state.counter.fetch_add(response.0, Ordering::Relaxed);
    }
    Ok(())
}
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.