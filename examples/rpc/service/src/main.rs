use std::time::Duration;

use ioevent::{
    state::{DefaultProcedureWright, ProcedureCallWright, procedure},
    prelude::*,
};
use rpc_common::*;
use tokio::{process::Command, select, time};

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
    let (
        Bus {
            mut center_ticker,
            mut subscribe_ticker,
            mut effect_ticker,
            mut shooter_ticker,
        },
        effect_wright,
    ) = builder.build();

    // Create application state
    let state = State::new(MyState::default(), effect_wright.clone());

    // Spawn a background task to periodically emit 'EmptyEvent'.
    let _handle = tokio::spawn(async move {
        loop {
            time::sleep(Duration::from_secs(1)).await;
            effect_wright.emit(&EmptyEvent).unwrap();
        }
    });

    let state_clone = state.clone();
    // Spawn the main event processing tasks.
    tokio::spawn(async move {
        loop {
            let errors = subscribe_ticker.tick(&state_clone).await;
            // **Important:** Consume the iterator to process all errors.
            for _ in errors {}
        }
    });
    tokio::spawn(async move {
        loop {
            shooter_ticker.tick(&state).await;
        }
    });
    // Main event loop processing ticks from various components.
    loop {
        select! {
            errors = effect_ticker.tick() => {
                // **Important:** Consume the iterator to process all effect errors.
                for _ in errors {}
            }
            errors = center_ticker.tick() => {
                // **Important:** Consume the iterator to process all center errors.
                for _ in errors {}
            }
        }
    }
}
