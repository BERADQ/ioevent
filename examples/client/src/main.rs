use ioevent::{
    bus::{Bus, BusBuilder, IoPair, state::State},
    create_subscriber,
    event::{Event, Subscriber, Subscribers},
    subscriber,
};
use serde::{Deserialize, Serialize};
use tokio::select;

static SUBSCRIBERS: &[Subscriber<MyState>] =
    &[create_subscriber!(echo), create_subscriber!(echo_c)];

#[derive(Clone)]
struct MyState;

#[derive(Deserialize, Serialize, Debug, Event)]
#[event(tag = "com::demo::my::A")]
struct A {
    foo: String,
    bar: i64,
}
#[derive(Deserialize, Serialize, Debug, Event)]
#[event(tag = "com::demo::my::B")]
struct B {
    foo: i64,
    bar: String,
}

#[derive(Deserialize, Serialize, Debug, Event)]
#[event(tag = "com::demo::my::C")]
struct C(String, i64);

#[subscriber]
async fn echo(s: State<MyState>, e: A) -> Result {
    s.bus.emit(&C(e.foo, e.bar))?;
    Ok(())
}

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
    let subscribes = Subscribers::init(SUBSCRIBERS);
    let mut builder = BusBuilder::new(subscribes);
    builder.add_pair(IoPair::stdio());
    let Bus {
        mut subscribe_ticker,
        mut effect_ticker,
        effect_wright,
    } = builder.build();
    let state = State::new(MyState, effect_wright.clone());
    loop {
        select! {
            _ = subscribe_ticker.tick(&state) => {},
            _ = effect_ticker.tick() => {},
        }
    }
}
