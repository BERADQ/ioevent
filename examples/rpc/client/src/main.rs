use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use ioevent::{
    State,
    bus::state::{DefaultProcedureWright, ProcedureCallExt, ProcedureCallWright},
    prelude::*,
};
use rpc_common::*;
use tokio::select;

// Define subscribers for handling events
const SUBSCRIBERS: &[Subscriber<MyState>] = &[create_subscriber!(call_print)];

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

#[subscriber]
async fn call_print(state: State<MyState>, _e: EmptyEvent) -> Result {
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
    let state = State::new(MyState::default(), effect_wright);

    // Main event loop for handling events
    let state_clone = state.clone();
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
    loop {
        select! {
            _ = effect_ticker.tick() => {}
            _ = center_ticker.tick() => {}
        }
    }
}
