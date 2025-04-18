use ioevent::prelude::*;
use base_common::*;
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

// Subscriber that broadcasts received events
#[subscriber]
async fn boardcast(s: State<MyState>, event: EventData) -> Result {
    s.bus.emit(&event)?;
    Ok(())
}

// Event structure for call data
#[derive(Deserialize, Serialize, Debug, Event)]
pub struct CallData {
}

#[tokio::main]
async fn main() {
    // Initialize subscribers and create a new client process
    let subscribes = Subscribers::init(SUBSCRIBERS);
    let child = Command::new("./client.exe");
    
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
    let state = State::new(MyState, effect_wright.clone());
    
    // Main event loop
    loop {
        let effect_wright = effect_wright.clone();
        // Periodic event sender that emits event A every second
        let send_event = async move {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            effect_wright
                .emit(&A {
                    foo: "hello".to_string(),
                    bar: 1,
                })
                .unwrap();
        };
        
        // Select between different async operations
        select! {
            _ = subscribe_ticker.tick(&state) => {},  // Handle subscription events
            _ = effect_ticker.tick() => {},          // Handle effect events
            _ = send_event => {},                    // Handle periodic event sending
        }
    }
}
