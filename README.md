# `IOEVENT`

[![GitHub License](https://img.shields.io/github/license/BERADQ/ioevent)](https://github.com/BERADQ/ioevent/blob/main/LICENSE)
![GitHub Issues or Pull Requests](https://img.shields.io/github/issues/BERADQ/ioevent)
![Crates.io Version](https://img.shields.io/crates/v/ioevent)

**Transform Any tokio Async I/O into Event-Driven Architecture with Low Overhead.**

A lightweight Rust crate built on Tokio's async runtime, providing a flexible event bus abstraction for asynchronous I/O operations. Perfect for building decoupled, event-driven systems with optimized performance.

## Features ✨
- 🚀 Event-driven Architecture: Convert async I/O operations into unified event streams
- 🔗 Tokio-powered: Seamless integration with Tokio's async ecosystem
- 🧩 Extensible: Custom event types and handler registration
- 🔄 Bi-directional Communication: Support both event emission and response handling

## Usage 🚀

**See The [Examples](https://github.com/BERADQ/ioevent/tree/main/examples)**

**Define Events**
```rust
#[derive(Deserialize, Serialize, Event)]
struct EventA {
    foo: String,
    bar: i64,
}

#[derive(Deserialize, Serialize, Debug, Event)]
struct EventB {
    foo: i64,
    bar: String,
}
```

**Create & Collect Subscribers**
```rust
static SUBSCRIBERS: &[Subscriber<()>] = &[
    create_subscriber!(echo),
    create_subscriber!(print_b)
];

#[subscriber]
async fn echo<T>(s: State<T>, e: EventA) -> Result {
    s.bus.emit(&EventB {
        foo: e.bar,
        bar: e.foo,
    })?;
    Ok(())
}
// outher side: without state or ret
#[subscriber]
async fn print_b(e: EventB) {
    println!("{:?}", e);
}
// custom event query
// ... see example
// merge event and state
// ... see example
```

**Create a Bus**
```rust
let subscribes = Subscribers::init(SUBSCRIBERS);
let mut builder = BusBuilder::new(subscribes);
builder.add_pair(IoPair::stdio());
let Bus {
    mut subscribe_ticker,
    mut effect_ticker,
    effect_wright,
} = builder.build();
```

**Create a State**
```rust
let state = State::new((), effect_wright.clone());
```

**Tick the Bus**
```rust
loop {
    select! {
        _ = subscribe_ticker.tick(&state) => {},
        _ = effect_ticker.tick() => {},
    }
}
```

## todo
- [ ] middleware support
- [ ] custom serializer & deserializer