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

// Define subscribers responsible for handling specific events.
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
    // Initiate an RPC call to the 'CallPrint' procedure with the current counter value.
    let call = state
        .call(&CallPrint(format!(
            "{}",
            state.counter.load(Ordering::Relaxed)
        )))
        .await;

    // If the RPC call succeeds, update the counter with the response value.
    if let Ok(call) = call {
        state.counter.fetch_add(call.0, Ordering::Relaxed);
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    // Initialize subscribers and configure the event bus builder.
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

    // Spawn the main event processing tasks.
    let state_clone = state.clone();
    tokio::spawn(async move {
        loop {
            let errors = subscribe_ticker.tick(&state_clone).await;
            // **Important:** Consume the iterator to process all errors.
            for _ in errors {}
        }
    });
    tokio::spawn(async move {
        loop {
            sooter_ticker.tick(&state).await;
        }
    });
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
