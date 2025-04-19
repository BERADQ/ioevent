# `IOEVENT`

[![GitHub License](https://img.shields.io/github/license/BERADQ/ioevent)](https://github.com/BERADQ/ioevent/blob/main/LICENSE)
![GitHub Issues or Pull Requests](https://img.shields.io/github/issues/BERADQ/ioevent)
![Crates.io Version](https://img.shields.io/crates/v/ioevent)
[![docs.rs](https://img.shields.io/docsrs/ioevent)](https://docs.rs/ioevent/latest/ioevent)

**Easily Transform Tokio Async I/O into an Event-Driven Architecture with Low Overhead.**

A lightweight Rust crate built on Tokio's async runtime, providing a flexible event bus abstraction for asynchronous I/O operations. Perfect for building decoupled, event-driven systems with optimized performance, especially useful for inter-process communication or modular application design.

## Features ✨
- 🚀 Event-driven Architecture: Convert async I/O operations into unified event streams
- 🔗 Tokio-powered: Seamless integration with Tokio's async ecosystem
- 🧩 Extensible: Custom event types and handler registration
- 🔄 Bi-directional Communication: Supports event emission and response handling (via Procedure Calls).

## Usage 🚀

**See The [Examples](https://github.com/BERADQ/ioevent/tree/main/examples)**

**Define Events**
```rust
use serde::{Deserialize, Serialize};
use ioevent::prelude::*;

// Define events based on examples/base/common
#[derive(Deserialize, Serialize, Debug, Event)]
pub struct A {
    pub foo: String,
    pub bar: i64,
}
#[derive(Deserialize, Serialize, Debug, Event)]
pub struct B {
    pub foo: i64,
    pub bar: String,
}
```

**Create & Collect Subscribers**
```rust
use ioevent::prelude::*; // Assuming other necessary imports like State, Result, EventData

// Define a simple application state structure.
// This can hold any data your subscribers might need.
#[derive(Clone)]
struct MyState;

// Define subscribers based on examples/base/host
// Subscribers are functions that react to specific events.
static SUBSCRIBERS: &[Subscriber<MyState>] = &[
    create_subscriber!(show_b),
    create_subscriber!(boardcast)
];

// Subscriber that handles event B and prints it to the console.
#[subscriber]
async fn show_b(event: B) {
    println!("Received and printing event B: {:?}", event);
}

// Subscriber that receives any event (EventData) and re-emits it onto the bus.
// It gets access to the application state (s: State<MyState>).
#[subscriber]
async fn boardcast(s: State<MyState>, event: EventData) -> Result {
    println!("Boardcasting event: {:?}", event.tag());
    s.bus.emit(&event)?; // Use the bus handle within the state to emit.
    Ok(())
}

// You can define more subscribers for other events...
```

**Create a Bus**
```rust
use tokio::process::Command; // Required for Command
use ioevent::prelude::*; // Assuming other necessary imports like BusBuilder, IoPair

// Assuming SUBSCRIBERS is defined as above
let subscribes = Subscribers::init(SUBSCRIBERS);
let mut builder = BusBuilder::new(subscribes);

// Add a communication channel (pair).
// Example 1: Connect to a child process (like in examples/base/host)
let child = Command::new("./base-client.exe"); // Adjust path as needed
builder.add_pair(child.try_into().unwrap());

// Example 2: Use standard input/output (often used in client examples)
// builder.add_pair(IoPair::stdio());

// Build the bus. This returns:
// - The `bus` object itself, which contains the internal tickers.
// - An `effect_wright` handle to send events *into* the bus from outside the subscribers.
let (bus, effect_wright) = builder.build();
```

**Create a State**
```rust
use ioevent::State;

// Assuming MyState and effect_wright are defined as above
// Create the application state, associating it with the effect_wright handle.
let state = State::new(MyState, effect_wright.clone());
```

**Run the Bus**
```rust
use ioevent::{Error, Result}; // Assuming Event A is defined
use chrono::Utc; // For timestamp generation
use std::time::Duration;

// Define an error handler closure.
// This function will be called if errors occur during bus operation.
let error_handler = |errors: Vec<Error>| {
    for error in errors {
        eprintln!("Bus Error: {:?}", error);
    }
};

// Run the event bus. This starts the internal event loop.
// It takes the application state and the error handler.
// The `bus.run` function typically runs indefinitely until an error occurs or the process exits.
// Note: bus.run returns a handle, which can be awaited if needed (e.g., in main).
let handle = bus.run(state, &error_handler);

// Example: Spawn a separate Tokio task to periodically send events into the bus.
// This demonstrates using the `effect_wright` handle obtained earlier.
tokio::spawn(async move {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let event_to_send = A {
            foo: "hello from periodic task".to_string(),
            bar: Utc::now().timestamp(), // Generate a timestamp
        };
        println!("Sending event A: {:?}", event_to_send);
        // Use the effect_wright handle to emit the event.
        if let Err(e) = effect_wright.emit(&event_to_send) {
             eprintln!("Failed to send periodic event: {:?}", e);
        }
     }
});

// Typically, you would await the handle in your main function
// to keep the application running.
// handle.await.await; // This might require handling the result
```

**Procedure Call**

**See The [Examples](https://github.com/BERADQ/ioevent/tree/main/examples/rpc)**

## todo
- [ ] middleware support
- [ ] custom serializer & deserializer
- [x] procedure call