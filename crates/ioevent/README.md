# `IOEVENT`

[![GitHub License](https://img.shields.io/github/license/BERADQ/ioevent)](https://github.com/BERADQ/ioevent/blob/main/LICENSE)
![GitHub Issues or Pull Requests](https://img.shields.io/github/issues/BERADQ/ioevent)
![Crates.io Version](https://img.shields.io/crates/v/ioevent)
[![docs.rs](https://img.shields.io/docsrs/ioevent)](https://docs.rs/ioevent/latest/ioevent)

A lightweight Rust crate for building event-driven applications on top of Tokio's async I/O streams with low overhead. Facilitates decoupled architectures, suitable for inter-process communication or modular designs.

## Features ✨
- **Event-driven Architecture**: Transforms async I/O operations into unified event streams.
- **Tokio Integration**: Built upon and integrates seamlessly with Tokio's async runtime.
- **Extensibility**: Supports custom event types and dynamic handler registration.
- **Bi-directional Communication**: Enables event emission and response handling through Procedure Calls.

## Usage 🚀

Refer to the [Examples](https://github.com/BERADQ/ioevent/tree/main/examples) directory for complete code samples.

**1. Define Events**

Events are simple structs or enums implementing `serde::Serialize`, `serde::Deserialize`, and deriving `ioevent::Event`.

```rust
use serde::{Deserialize, Serialize};
use ioevent::prelude::*;

// Example events (similar to examples/base/common)
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

// Define an application state (optional)
#[derive(Clone)]
struct AppState { /* ... fields needed by subscribers ... */ }

// Subscriber functions (similar to examples/base/host)
#[subscriber]
async fn handle_ping(state: State<AppState>, event: Ping) -> Result {
    // println!("Received Ping: {:?}", event);
    // Respond with a Pong event using the bus handle from the state
    let pong = Pong { timestamp: event.timestamp };
    state.bus.emit(&pong)?;
    Ok(())
}

#[subscriber]
async fn log_event(event: EventData) { // Receives any event
    println!("Logged Event Tag: {:?}", event.tag());
}

// Collect subscribers into a static slice
static SUBSCRIBERS: &[Subscriber<AppState>] = &[
    create_subscriber!(handle_ping),
    create_subscriber!(log_event),
];
```

**3. Build the Event Bus**

Use `BusBuilder` to configure and create the event bus, adding communication pairs (e.g., `stdio` or child process I/O).

```rust
use tokio::process::Command;
use ioevent::prelude::*;

fn setup_bus() -> (Bus<AppState>, EffectWright<AppState>) {
    let subscribers = Subscribers::init(SUBSCRIBERS);
    let mut builder = BusBuilder::new(subscribers);

    // Example 1: Standard Input/Output (common for client/server)
    // builder.add_pair(IoPair::stdio());

    // Example 2: Child Process Communication (host example)
    let mut child_cmd = Command::new("path/to/your/client/executable"); // Adjust path
    // Configure stdin/stdout redirection as needed for Command
    // ...
    if let Ok(pair) = IoPair::try_from(child_cmd) {
         builder.add_pair(pair);
    } else {
        eprintln!("Failed to create child process pair");
    }


    // Build the bus and get the effect handle
    builder.build()
}
```

**4. Create State (if needed)**

Instantiate your application state and associate it with the `EffectWright` handle obtained from `builder.build()`.

```rust
use ioevent::State; // Assuming AppState and effect_wright are defined

let (bus, effect_wright) = setup_bus().await; // Assuming setup_bus is defined
let state = State::new(AppState { /* ... initial state ... */ }, effect_wright.clone());
```

**5. Run the Bus**

Start the event processing loop using `bus.run()`. Provide the application state and an error handler.

```rust
use ioevent::{Error, Result};
use std::time::Duration;

// Assuming bus, state, effect_wright are defined
// Define an error handler closure
let error_handler = |errors: Vec<Error>| {
    for error in errors {
        eprintln!("Bus Error: {:?}", error);
        // Add more robust error handling/logging as needed
    }
};

// Run the event bus (typically runs indefinitely)
let bus_handle = bus.run(state, &error_handler);

// Example: Spawn a task to periodically send an event
let periodic_sender = effect_wright.clone(); // Clone handle for the task
tokio::spawn(async move {
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let event = Ping { timestamp: 0 }; 
        if let Err(e) = periodic_sender.emit(&event) {
            eprintln!("Failed to send periodic event: {:?}", e);
        }
    }
});

// Keep the application alive, e.g., by awaiting the bus handle
// (Note: bus.run itself might block or return a future to await)
// bus_handle.await.unwrap(); // Handle potential errors from the bus run
```

## Procedure Call (RPC)

`ioevent` supports request/response patterns via Procedure Calls. Define procedures similarly to events and use `bus.call()` to invoke them remotely.

**See the [RPC Examples](https://github.com/BERADQ/ioevent/tree/main/examples/rpc) for details.**

## TODO
- [ ] Middleware support
- [ ] Custom serializer & deserializer
- [x] Procedure call