use base_common::*;
use ioevent::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::{process::Command, select};

// Define subscribers that will handle specific events
static SUBSCRIBERS: &[Subscriber<MyState>] =
    &[create_subscriber!(show_b), create_subscriber!(boardcast)];

// Application state structure
#[derive(Clone)]
struct MyState;

// Subscriber that prints event B to console
#[subscriber]
async fn show_b(event: B) {
    println!("{:?}", event);
}

// Subscriber that re-emits received events onto the bus
#[subscriber]
async fn boardcast(s: State<MyState>, event: EventData) -> Result {
    s.bus.emit(&event)?;
    Ok(())
}

// Event structure for call data
#[derive(Deserialize, Serialize, Debug, Event)]
pub struct CallData {}

#[tokio::main]
async fn main() {
    // Initialize subscribers and create a new client process
    let subscribes = Subscribers::init(SUBSCRIBERS);
    let child = Command::new("./base-client.exe");

    // Build the event bus with subscribers and client connection
    let mut builder = BusBuilder::new(subscribes);
    builder.add_pair(child.try_into().unwrap());

    // Initialize the event bus components
    let (bus, effect_wright) = builder.build();

    // Create application state
    let state = State::new(MyState, effect_wright.clone());

    let handle = bus.run(state, &|errors| {
        for error in errors {
            eprintln!("error: {:?}", error);
        }
    });
    // Spawn a task to periodically send event A
    tokio::spawn(async move {
        loop {
            // Periodic event sender that emits event A every second
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            effect_wright
                .emit(&A {
                    foo: "hello".to_string(),
                    bar: 1,
                })
                .unwrap();
        }
    });
    handle.await.await;
}
