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

const SUBSCRIBERS: &[Subscriber<MyState>] = &[];

#[derive(Clone, Default)]
struct MyState {
    pdc_wright: DefaultProcedureWright,
    counter: Arc<AtomicU64>,
}
impl ProcedureCallWright for MyState {
    async fn next_echo(&self) -> u64 {
        self.pdc_wright.next_echo().await
    }
}

#[tokio::main]
async fn main() {
    let subscribes = Subscribers::init(SUBSCRIBERS);
    let mut builder = BusBuilder::new(subscribes);
    builder.add_pair(IoPair::stdio());
    let Bus {
        mut subscribe_ticker,
        mut effect_ticker,
        effect_wright,
    } = builder.build();
    let state = State::new(MyState::default(), effect_wright.clone());

    let state_ = state.clone();
    tokio::spawn(async move {
        let state = state_.clone();
        loop {
            time::sleep(Duration::from_millis(1000)).await;

            let call = state
                .call(&CallPrint(format!(
                    "{}",
                    state.counter.load(Ordering::Relaxed)
                )))
                .await;

            if let Ok(call) = call {
                state.counter.fetch_add(call.0, Ordering::Relaxed);
            }
        }
    });

    loop {
        select! {
            _ = subscribe_ticker.tick(&state) => {},
            _ = effect_ticker.tick() => {},
        }
    }
}
