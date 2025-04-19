use base_common::*;
use ioevent::prelude::*;
use tokio::select;

// Define subscribers for handling events A and C
static SUBSCRIBERS: &[Subscriber<MyState>] =
    &[create_subscriber!(echo), create_subscriber!(echo_c)];

// Client application state
#[derive(Clone)]
struct MyState;

// Subscriber that handles event A and emits event C
#[subscriber]
async fn echo(s: State<MyState>, e: A) -> Result {
    s.bus.emit(&C(e.foo, e.bar))?;
    Ok(())
}

// Subscriber that handles event C and emits event B
#[subscriber]
async fn echo_c<T: Clone>(state: State<T>, event: C) -> Result {
    state.bus.emit(&B {
        foo: event.1,
        bar: event.0,
    })?;
    Ok(())
}

#[tokio::main]
async fn main() {
    // Initialize subscribers and create event bus
    let subscribes = Subscribers::init(SUBSCRIBERS);
    let mut builder = BusBuilder::new(subscribes);

    // Add standard I/O as communication channel
    builder.add_pair(IoPair::stdio());

    // Initialize event bus components
    let (bus, effect_wright) = builder.build();

    // Create application state
    let state = State::new(MyState, effect_wright);
    let handle = bus.run(state, &|errors| {
        for error in errors {
            eprintln!("error: {:?}", error);
        }
    });
    handle.await.await;
}
