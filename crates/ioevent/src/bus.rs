use channels::{
    io::{AsyncRead, AsyncWrite, IntoRead, IntoWrite},
    serdes::Cbor,
};
use futures::future::{self, join_all};
use serde::{Deserialize, Serialize};
use state::State;
use tokio::task;

use crate::{
    error::{BusRecvError, BusSendError, CallSubscribeError, TryFromEventError},
    event::*,
};

/// Ticker for subscribing to events.
pub struct SubscribeTicker<T: 'static, R> {
    pub subs: Subscribers<T>,
    pub rx: Vec<channels::Receiver<EventData, R, Cbor>>,
}

impl<T, R> SubscribeTicker<T, R>
where
    T: 'static,
    R: AsyncRead + Unpin,
{
    /// Receive one event from all receivers and emit it to all subscribers.
    ///
    /// The returned iterator is an iterator over errors that
    /// occurred while emitting events to subscribers. If any receiver returned `Ok`
    /// , the iterator will be empty.
    ///
    /// If all subscribers failed to receive an event, the error is returned in `Err`.
    ///
    /// This method is `Cancel Safety`.
    pub async fn tick(
        &mut self,
        state: &State<T>,
    ) -> Result<impl Iterator<Item = CallSubscribeError>, BusRecvError<R::Error>> {
        let iter = self.rx.iter_mut().map(|a| Box::pin(a.recv()));
        let result = future::select_ok(iter).await;

        match result {
            Ok((e, v)) => {
                drop(v);
                Ok(self.subs.emit(state, &e).await)
            }
            Err(e) => Err(e.into()),
        }
    }
}

/// Ticker for sending events.
pub struct EffectTicker<W> {
    pub tx: Vec<channels::Sender<EventData, W, Cbor>>,
    pub state_rx: tokio::sync::mpsc::UnboundedReceiver<EventData>,
}

impl<W> EffectTicker<W>
where
    W: AsyncWrite + Unpin,
{
    /// Receive one event from effect channel and emit it to all subscribers.
    ///
    /// The returned iterator is an iterator over errors that
    /// occurred while emitting events to subscribers. If any receiver returned `Ok`
    /// , the iterator will be empty.
    ///
    /// This method is `Cancel Safety`.
    pub async fn tick(&mut self) -> impl Iterator<Item = BusSendError<W::Error>> {
        let event = self.state_rx.recv().await;
        if let Some(event) = event {
            let results = join_all(self.tx.iter_mut().map(|t| t.send(event.clone())));
            let results = task::unconstrained(results).await;
            let results = results.into_iter().filter_map(Result::err).map(Into::into);
            Some(results)
        } else {
            None
        }
        .into_iter()
        .flatten()
    }
}

/// Wright for sending events.
#[derive(Clone)]
pub struct EffectWright {
    pub state_tx: tokio::sync::mpsc::UnboundedSender<EventData>,
}

impl EffectWright {
    /// Send an event to the effect channel.
    pub fn emit<E>(&self, event: &E) -> Result<(), CallSubscribeError>
    where
        E: Event,
    {
        let event = event.upcast()?;
        self.state_tx.send(event)?;
        Ok(())
    }
}

/// A bus for sending and receiving events.
pub struct Bus<T, W, R>
where
    T: 'static,
{
    pub subscribe_ticker: SubscribeTicker<T, R>,
    pub effect_ticker: EffectTicker<W>,
    pub effect_wright: EffectWright,
}

/// A pair of reader and writer.
pub struct IoPair<IR, IW> {
    pub reader: IR,
    pub writer: IW,
}

impl IoPair<tokio::io::Stdin, tokio::io::Stdout> {
    /// Get a pair of stdin and stdout
    pub fn stdio() -> Self {
        IoPair {
            reader: tokio::io::stdin(),
            writer: tokio::io::stdout(),
        }
    }
}

impl TryFrom<tokio::process::Child>
    for IoPair<tokio::process::ChildStdout, tokio::process::ChildStdin>
{
    type Error = ();
    fn try_from(mut value: tokio::process::Child) -> Result<Self, Self::Error> {
        let (child_stdin, child_stdout) = (value.stdin.take(), value.stdout.take());
        if let (Some(child_stdin), Some(child_stdout)) = (child_stdin, child_stdout) {
            Ok(IoPair {
                reader: child_stdout,
                writer: child_stdin,
            })
        } else {
            Err(())
        }
    }
}

impl TryFrom<tokio::process::Command>
    for IoPair<tokio::process::ChildStdout, tokio::process::ChildStdin>
{
    type Error = std::io::Error;
    fn try_from(mut value: tokio::process::Command) -> Result<Self, Self::Error> {
        value
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped());
        let child = value.spawn()?;
        child
            .try_into()
            .map_err(|_| std::io::Error::last_os_error())
    }
}

/// A builder for a bus.
pub struct BusBuilder<T, W, R>
where
    T: 'static,
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
{
    subs: Subscribers<T>,
    rx: Vec<channels::Receiver<EventData, R, Cbor>>,
    tx: Vec<channels::Sender<EventData, W, Cbor>>,
    state_rx: tokio::sync::mpsc::UnboundedReceiver<EventData>,
    state_tx: tokio::sync::mpsc::UnboundedSender<EventData>,
}

impl<T, W, R> BusBuilder<T, W, R>
where
    T: 'static,
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
{
    /// Create a new bus builder.
    pub fn new(subscribes: Subscribers<T>) -> Self {
        let (state_tx, state_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            subs: subscribes,
            rx: Vec::new(),
            tx: Vec::new(),
            state_rx,
            state_tx,
        }
    }
    /// Add a reader to the bus.
    pub fn add_reader<IR>(&mut self, reader: IR) -> &mut Self
    where
        IR: IntoRead<R>,
    {
        let rx = channels::Receiver::<EventData, _, _>::builder()
            .reader(reader)
            .deserializer(Cbor::new())
            .build();
        self.rx.push(rx);
        self
    }
    /// Add a writer to the bus.
    pub fn add_sender<IW>(&mut self, writer: IW) -> &mut Self
    where
        IW: IntoWrite<W>,
    {
        let rx = channels::Sender::<EventData, _, _>::builder()
            .writer(writer)
            .serializer(Cbor::new())
            .build();
        self.tx.push(rx);
        self
    }
    /// Add a pair of reader and writer to the bus.
    pub fn add_pair<IR, IW>(&mut self, pair: IoPair<IR, IW>) -> &mut Self
    where
        IR: IntoRead<R>,
        IW: IntoWrite<W>,
    {
        let IoPair { reader, writer } = pair.into();
        self.add_reader(reader);
        self.add_sender(writer);
        self
    }
    /// Build the bus.
    pub fn build(self) -> Bus<T, W, R> {
        Bus {
            subscribe_ticker: SubscribeTicker {
                subs: self.subs,
                rx: self.rx,
            },
            effect_ticker: EffectTicker {
                tx: self.tx,
                state_rx: self.state_rx,
            },
            effect_wright: EffectWright {
                state_tx: self.state_tx,
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcedureCallData {
    pub echo: u64,
    pub data: ciborium::Value,
}
impl Event for ProcedureCallData {
    const TAG: &'static str = "#ProcedureCallData::";
    const SELECTOR: Selector = Selector(|x| x.event.starts_with(Self::TAG));
    fn upcast(&self) -> Result<EventData, ciborium::value::Error> {
        Ok(EventData {
            event: Self::TAG.to_string() + &self.echo.to_string(),
            data: self.data.clone(),
        })
    }
}
impl TryFrom<&EventData> for ProcedureCallData {
    type Error = TryFromEventError;
    fn try_from(value: &EventData) -> Result<Self, Self::Error> {
        Ok(ProcedureCallData {
            echo: value.event.replace(Self::TAG, "").parse().map_err(
                |e: std::num::ParseIntError| TryFromEventError::InvalidEvent(e.to_string()),
            )?,
            data: value.data.clone(),
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcedureCallRetData {
    pub echo: u64,
    pub data: ciborium::Value,
}
impl Event for ProcedureCallRetData {
    const TAG: &'static str = "#ProcedureCallRetData::";
    const SELECTOR: Selector = Selector(|x| x.event.starts_with(Self::TAG));
    fn upcast(&self) -> Result<EventData, ciborium::value::Error> {
        Ok(EventData {
            event: Self::TAG.to_string() + &self.echo.to_string(),
            data: self.data.clone(),
        })
    }
}
impl TryFrom<&EventData> for ProcedureCallRetData {
    type Error = TryFromEventError;
    fn try_from(value: &EventData) -> Result<Self, Self::Error> {
        Ok(ProcedureCallRetData {
            echo: value.event.replace(Self::TAG, "").parse().map_err(
                |e: std::num::ParseIntError| TryFromEventError::InvalidEvent(e.to_string()),
            )?,
            data: value.data.clone(),
        })
    }
}

pub mod state {
    use std::collections::HashMap;

    use rand::{RngCore, rngs::mock::StepRng};
    use tokio::sync::{Mutex, RwLock, oneshot};

    use crate::{Event, EventData, error::CallSubscribeError, future::SubscribeFutureRet};

    use super::{EffectWright, ProcedureCallData, ProcedureCallRetData};

    /// The state of the bus. For collect side effects.
    #[derive(Clone)]
    pub struct State<T> {
        pub state: T,
        pub bus: EffectWright,
    }
    impl<T> State<T> {
        /// Create a new state.
        pub fn new(state: T, bus: EffectWright) -> Self {
            Self { state, bus }
        }
    }
    pub trait ProcedureCall {
        fn wait(
            &self,
            echo: u64,
        ) -> impl Future<Output = Result<ciborium::Value, CallSubscribeError>>;
        fn resolve(
            &self,
            echo: u64,
            data: ciborium::Value,
        ) -> impl Future<Output = Result<(), CallSubscribeError>>;
        fn try_resolve(
            &self,
            echo: u64,
            data: ciborium::Value,
        ) -> impl Future<Output = Result<(), CallSubscribeError>>;
        fn generate_echo(&self) -> impl Future<Output = u64>;
    }

    pub trait MayProcedureCall {
        fn get_procedure_call_state(&self) -> &ProcedureCallState;
    }

    impl<T> ProcedureCall for T
    where
        T: MayProcedureCall,
    {
        async fn wait(&self, echo: u64) -> Result<ciborium::Value, CallSubscribeError> {
            let state = self.get_procedure_call_state();
            let (tx, rx) = oneshot::channel();
            state.channel_shot.write().await.insert(echo, tx);
            let result = rx.await.map_err(|_| {
                CallSubscribeError::ProcedureCall(format!("Procedure call {} failed", echo))
            })?;
            Ok(result)
        }
        async fn resolve(
            &self,
            echo: u64,
            data: ciborium::Value,
        ) -> Result<(), CallSubscribeError> {
            let state = self.get_procedure_call_state();
            let tx = state.channel_shot.write().await.remove(&echo);
            if let Some(tx) = tx {
                tx.send(data).map_err(|_| {
                    CallSubscribeError::ProcedureCall(format!("Procedure call {} failed", echo))
                })?;
                Ok(())
            } else {
                Err(CallSubscribeError::ProcedureCall(format!(
                    "Procedure call {} not found",
                    echo
                )))
            }
        }
        async fn try_resolve(
            &self,
            echo: u64,
            data: ciborium::Value,
        ) -> Result<(), CallSubscribeError> {
            let state = self.get_procedure_call_state();
            let tx = state.channel_shot.read().await.contains_key(&echo);
            if tx {
                self.resolve(echo, data).await
            } else {
                Ok(())
            }
        }
        async fn generate_echo(&self) -> u64 {
            let mut rand = self.get_procedure_call_state().rand.lock().await;
            rand.next_u64()
        }
    }

    pub trait ProcedureCallExt {
        fn procedure_call(
            &self,
            data: ciborium::Value,
        ) -> impl Future<Output = Result<ciborium::Value, CallSubscribeError>>;
    }
    impl<T> ProcedureCallExt for State<T>
    where
        T: ProcedureCall,
    {
        async fn procedure_call(
            &self,
            data: ciborium::Value,
        ) -> Result<ciborium::Value, CallSubscribeError> {
            let echo = self.state.generate_echo().await;
            let data = ProcedureCallData { echo, data }.upcast()?;
            let result = self.state.wait(echo).await?;
            self.bus.emit(&data)?;
            Ok(result)
        }
    }
    pub fn procedure_ret_subscribe<T>(state: &State<T>, event: &EventData) -> SubscribeFutureRet
    where
        T: ProcedureCall + Clone + 'static,
    {
        let pcd = ProcedureCallRetData::try_from(event);
        let state = state.clone();
        Box::pin(async move {
            let pcd = pcd?;
            state.state.resolve(pcd.echo, pcd.data).await?;
            Ok(())
        })
    }
    #[doc(hidden)]
    pub mod procedure_ret_subscribe {
        use crate::bus::ProcedureCallRetData;
        pub type _Event = ProcedureCallRetData;
    }
    pub struct ProcedureCallState {
        pub rand: Mutex<StepRng>,
        pub channel_shot: RwLock<HashMap<u64, oneshot::Sender<ciborium::Value>>>,
    }
    impl MayProcedureCall for ProcedureCallState {
        fn get_procedure_call_state(&self) -> &ProcedureCallState {
            self
        }
    }
}
