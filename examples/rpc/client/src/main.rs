use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use ioevent::{
    State,
    bus::state::{DefaultProcedureWright, ProcedureCallExt, ProcedureCallWright},
    prelude::*,
};
use rpc_common::*;
use tokio::{select, time};

// Define subscribers for handling events
const SUBSCRIBERS: &[Subscriber<MyState>] = &[];

// Application state with procedure call support and counter
#[derive(Clone, Default)]
struct MyState {
    pdc_wright: DefaultProcedureWright,
    counter: Arc<AtomicU64>,
}

// Implement procedure call functionality for the state
impl ProcedureCallWright for MyState {
    async fn next_echo(&self) -> u64 {
        self.pdc_wright.next_echo().await
    }
}

#[tokio::main]
async fn main() {
    // Initialize subscribers and create event bus
    let subscribes = Subscribers::init(SUBSCRIBERS);
    let mut builder = BusBuilder::new(subscribes);
    
    // Add standard I/O as communication channel
    builder.add_pair(IoPair::stdio());
    
    // Initialize event bus components
    let Bus {
        mut subscribe_ticker,
        mut effect_ticker,
        effect_wright,
    } = builder.build();
    
    // Create application state
    let state = State::new(MyState::default(), effect_wright.clone());

    // Spawn a background task for periodic RPC calls
    let state_ = state.clone();
    tokio::spawn(async move {
        let state = state_.clone();
        loop {
            // Wait for 1 second between calls
            time::sleep(Duration::from_millis(1000)).await;

            // Make RPC call to print the current counter value
            let call = state
                .call(&CallPrint(format!(
                    "{}",
                    state.counter.load(Ordering::Relaxed)
                )))
                .await;

            // If call is successful, update the counter with the response value
            if let Ok(call) = call {
                state.counter.fetch_add(call.0, Ordering::Relaxed);
            }
        }
    });

    // Main event loop for handling events
    loop {
        select! {
            _ = subscribe_ticker.tick(&state) => {},  // Handle event subscriptions
            _ = effect_ticker.tick() => {},          // Handle effect events
        }
    }
}
