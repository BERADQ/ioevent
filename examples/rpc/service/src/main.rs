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

static SUBSCRIBERS: &[Subscriber<MyState>] = &[create_subscriber!(print_test)];

#[derive(Clone, Default)]
struct MyState {
    pdc_wright: DefaultProcedureWright,
}
impl ProcedureCallWright for MyState {
    async fn next_echo(&self) -> u64 {
        self.pdc_wright.next_echo().await
    }
}

#[procedure]
async fn print_test(e: CallPrint) -> Result {
    println!("{}", e.0);
    Ok(CallPrintResponse(42))
}

#[tokio::main]
async fn main() {
    let subscribes = Subscribers::init(SUBSCRIBERS);
    let child = Command::new("./rpc-client.exe");
    let mut builder = BusBuilder::new(subscribes);
    builder.add_pair(child.try_into().unwrap());
    let Bus {
        mut subscribe_ticker,
        mut effect_ticker,
        effect_wright,
    } = builder.build();
    let state = State::new(MyState::default(), effect_wright);
    loop {
        select! {
            _ = subscribe_ticker.tick(&state) => {},
            _ = effect_ticker.tick() => {},
        }
    }
}
