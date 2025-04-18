use ioevent::{
    State,
    bus::state::{
        DefaultProcedureWright, ProcedureCallExt, ProcedureCallRequest, ProcedureCallWright,
        procedure,
    },
    prelude::*,
};
use rpc_common::*;
use tokio::{process::Command, select};

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

// RPC procedure that prints the received message and returns a response
#[procedure]
async fn print_test(e: CallPrint) -> Result {
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
    let Bus {
        mut subscribe_ticker,
        mut effect_ticker,
        effect_wright,
    } = builder.build();
    
    // Create application state
    let state = State::new(MyState::default(), effect_wright);
    
    // Main event loop for handling events
    loop {
        select! {
            _ = subscribe_ticker.tick(&state) => {},  // Handle event subscriptions
            _ = effect_ticker.tick() => {},          // Handle effect events
        }
    }
}
