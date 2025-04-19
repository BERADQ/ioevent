use std::time::Duration;

use ioevent::{
    State,
    bus::state::{
        DefaultProcedureWright, ProcedureCallExt, ProcedureCallRequest, ProcedureCallWright,
        procedure,
    },
    prelude::*,
};
use rpc_common::*;
use tokio::{process::Command, select, time};

// Define subscribers for handling events
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

// RPC procedure handling CallPrint request, prints the message and returns a fixed response
#[procedure]
async fn print_test(e: CallPrint) -> Result {
    println!("hello procedure");
    println!("{}", e.0);
    Ok(CallPrintResponse(42))
}

#[tokio::main]
async fn main() {
    // Initialize subscribers and create a new client process
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
            mut sooter_ticker,
        },
        effect_wright,
    ) = builder.build();

    // Create application state
    let state = State::new(MyState::default(), effect_wright.clone());

    // Spawn a task to periodically send EmptyEvent
    let _handle = tokio::spawn(async move {
        loop {
            time::sleep(Duration::from_secs(1)).await;
            effect_wright.emit(&EmptyEvent).unwrap();
        }
    });

    let state_clone = state.clone();
    // Main event loop for handling events
    tokio::spawn(async move {
        loop {
            let _ = subscribe_ticker.tick(&state_clone).await;
        }
    });
    tokio::spawn(async move {
        loop {
            let _ = sooter_ticker.tick(&state).await;
        }
    });
    // Main event loop combining different tickers
    loop {
        select! {
            _ = effect_ticker.tick() => {}
            _ = center_ticker.tick() => {}
        }
    }
}
