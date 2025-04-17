use ioevent::{
    State,
    bus::state::{
        DefaultProcedureWright, ProcedureCallData, ProcedureCallExt, ProcedureCallRequest,
        ProcedureCallWright,
    },
    subscriber,
    prelude::*
};
use rpc_common::*;
use tokio::{process::Command, select};

static SUBSCRIBERS: &[Subscriber<MyState>] = &[create_subscriber!(print)];

#[derive(Clone, Default)]
struct MyState {
    pdc_wright: DefaultProcedureWright,
}
impl ProcedureCallWright for MyState {
    async fn next_echo(&self) -> u64 {
        self.pdc_wright.next_echo().await
    }
}

#[subscriber]
async fn print(state: State<MyState>, e: ProcedureCallData) -> Result {
    if CallPrint::match_self(&e) {
        let echo = e.echo;
        let call = CallPrint::try_from(e)?;
        println!("{}", call.0);
        let response = CallPrintResponse(42);
        state.resolve::<CallPrint>(echo, &response).await?;
    }
    Ok(())
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
