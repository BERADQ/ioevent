use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use ioevent::{
    State,
    prelude::*,
    state::{DefaultProcedureWright, ProcedureCallExt, ProcedureCallWright},
};
use rpc_common::*;

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
    let (bus, effect_wright) = builder.build();

    // Create application state
    let state = State::new(MyState::default(), effect_wright);

    // Spawn the main event processing tasks.
    bus.run(state, &|e| {
        eprintln!("{:?}", e);
    }).await.join().await;
}
