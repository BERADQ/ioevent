use std::time::Duration;

use ioevent::{
    prelude::*,
    state::{DefaultProcedureWright, ProcedureCallWright, procedure},
};
use rpc_common::*;
use tokio::{process::Command, time};

// Define subscribers responsible for handling specific events.
static SUBSCRIBERS: &[Subscriber<MyState>] = &[create_subscriber!(print_test)];

// Application state with procedure call support
#[derive(Clone, Default)]
struct MyState {
    pdc_wright: DefaultProcedureWright,
}

// Implement procedure call functionality for the state
impl ProcedureCallWright for MyState {
    async fn next_echo(&self) -> u64 {
        self.pdc_wright.next_echo().await
    }
}

// Procedure implementation for handling 'CallPrint' requests.
// Prints the received message and returns a predefined response.
#[procedure]
async fn print_test(e: CallPrint) -> Result {
    println!("hello procedure");
    println!("{}", e.0);
    Ok(CallPrintResponse(42))
}

#[tokio::main]
async fn main() {
    // Initialize subscribers and prepare the client process command.
    let subscribes = Subscribers::init(SUBSCRIBERS);
    let child = Command::new("./rpc-client.exe");

    // Build the event bus with subscribers and client connection
    let mut builder = BusBuilder::new(subscribes);
    builder.add_pair(child.try_into().unwrap());

    // Initialize the event bus components
    let (bus, effect_wright) = builder.build();

    // Create application state
    let state = State::new(MyState::default(), effect_wright.clone());

    // Spawn a background task to periodically emit 'EmptyEvent'.
    let _handle = tokio::spawn(async move {
        loop {
            time::sleep(Duration::from_secs(1)).await;
            effect_wright.emit(&EmptyEvent).unwrap();
        }
    });

    bus.run(state, &|e| {
        eprintln!("{:?}", e);
    })
    .await
    .join()
    .await;
}
