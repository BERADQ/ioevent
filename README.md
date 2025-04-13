# `IOEVENT`

**Transform Any Async I/O into Event-Driven Architecture with Low Overhead**

A lightweight Rust crate built on Tokio's async runtime, providing a flexible event bus abstraction for asynchronous I/O operations. Perfect for building decoupled, event-driven systems with optimized performance.

## Features ✨
- 🚀 Event-driven Architecture: Convert async I/O operations into unified event streams
- 🔗 Tokio-powered: Seamless integration with Tokio's async ecosystem
- 📦 Lightweight: Minimal boilerplate and lean dependency tree
- 🧩 Extensible: Custom event types and handler registration
- 🔄 Bi-directional Communication: Support both event emission and response handling

## Usage 🚀

**See The [Examples](https://github.com/BERADQ/ioevent/tree/main/examples)**

**Define Events**
```rust
#[derive(Deserialize, Serialize, Debug, Event)]
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

**Collect Subscribers**
```rust
static SUBSCRIBERS: &[Subscriber<()>] = &[
    create_subscriber!(echo)
];

#[subscriber]
async fn echo(s: State<()>, e: EventA) -> Result {
    s.bus.emit(&EventB {
        foo: e.bar,
        bar: e.foo,
    })?;
    Ok(())
}
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

## todo
- [ ] middleware support
- [ ] custom serializer & deserializer