use ioevent::{
    bus::{state::State, Bus, BusBuilder},
    create_subscriber,
    event::{Event, EventData, Subscriber, Subscribers},
    subscriber,
};
use serde::{Deserialize, Serialize};
use tokio::{process::Command, select};

static SUBSCRIBERS: &[Subscriber<MyState>] =
    &[create_subscriber!(show_b), create_subscriber!(boardcast)];

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
pub struct B {
    foo: i64,
    bar: String,
}
#[derive(Deserialize, Serialize, Debug, Event)]
#[event(tag = "com::demo::my::C")]
struct C(String, i64);

#[subscriber]
async fn show_b(event: B) {
    println!("{:?}", event);
}
#[subscriber]
async fn boardcast(s: State<MyState>, event: EventData) -> Result {
    s.bus.emit(&event)?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let subscribes = Subscribers::init(SUBSCRIBERS);
    let child = Command::new("./client.exe");
    let mut builder = BusBuilder::new(subscribes);
    builder.add_pair(child.try_into().unwrap());
    let Bus {
        mut subscribe_ticker,
        mut effect_ticker,
        effect_wright,
    } = builder.build();
    let state = State::new(MyState, effect_wright.clone());
    loop {
        let effect_wright = effect_wright.clone();
        let send_event = async move {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            effect_wright
                .emit(&A {
                    foo: "hello".to_string(),
                    bar: 1,
                })
                .unwrap();
        };
        select! {
            _ = subscribe_ticker.tick(&state) => {},
            _ = effect_ticker.tick() => {},
            _ = send_event => {},
        }
    }
}
