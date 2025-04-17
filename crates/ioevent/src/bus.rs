use std::mem::swap;

use channels::{
    io::{AsyncRead, AsyncWrite, IntoRead, IntoWrite},
    serdes::Cbor,
};
use futures::future::{self, join_all};
use state::State;
use tokio::task;

use crate::{
    error::{BusRecvError, BusSendError, CallSubscribeError},
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
                let results = task::unconstrained(async {
                    drop(v);
                    let results = self.subs.emit(state, &e).await;
                    let mut event_shooters = state.event_shooters.lock().await;
                    let mut later: Vec<_> = Vec::with_capacity(event_shooters.len());
                    swap(&mut later, &mut event_shooters);
                    for st in later.into_iter() {
                        if let Some(st) = st.try_shoot_out(&e) {
                            event_shooters.push(st);
                        }
                    }
                    drop(event_shooters);
                    results
                })
                .await;
                Ok(results)
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
            let results = task::unconstrained(async {
                let results = join_all(self.tx.iter_mut().map(|t| t.send(event.clone())));
                let results = results.await;
                let results = results.into_iter().filter_map(Result::err).map(Into::into);
                results
            })
            .await;
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

pub mod state {
    use std::{ops::Deref, sync::Arc};

    use rand::{RngCore, SeedableRng, rngs::SmallRng};
    use serde::{Deserialize, Serialize};
    use tokio::sync::{Mutex, oneshot};

    use crate::{
        error::{CallSubscribeError, CborValueError, TryFromEventError},
        event::{self, Event, EventData},
    };

    use super::EffectWright;

    pub struct EventShooter {
        pub selector: Box<dyn Fn(&EventData) -> bool + Send + Sync + 'static>,
        pub shooter: oneshot::Sender<EventData>,
    }
    impl EventShooter {
        pub fn shoot_out_with(
            selector: Box<dyn Fn(&EventData) -> bool + Send + Sync + 'static>,
        ) -> (Self, oneshot::Receiver<EventData>) {
            let (shooter, receiver) = oneshot::channel();
            (Self { selector, shooter }, receiver)
        }
        pub fn try_shoot_out(self, event: &EventData) -> Option<Self> {
            if (self.selector)(event) {
                unsafe { self.shoot_out(event) };
                None
            } else {
                Some(self)
            }
        }
        pub unsafe fn shoot_out(self, event: &EventData) -> bool {
            self.shooter.send(event.clone()).is_ok()
        }
    }
    /// The state of the bus. For collect side effects.
    #[derive(Clone)]
    pub struct State<T> {
        pub state: T,
        pub bus: EffectWright,
        pub event_shooters: Arc<Mutex<Vec<EventShooter>>>,
    }
    impl<T> Deref for State<T> {
        type Target = T;
        fn deref(&self) -> &Self::Target {
            &self.state
        }
    }
    impl<T> State<T> {
        /// Create a new state.
        pub fn new(state: T, bus: EffectWright) -> Self {
            Self {
                state,
                bus,
                event_shooters: Arc::new(Mutex::new(Vec::new())),
            }
        }
        /// Wait for the next event.
        pub async fn wait_next<E>(&self) -> Result<E, CallSubscribeError>
        where
            E: Event,
        {
            let event = self.wait_next_with(E::SELECTOR.0).await?;
            Ok(E::try_from(&event)?)
        }
        pub async fn wait_next_with<F>(&self, selector: F) -> Result<EventData, CallSubscribeError>
        where
            F: Fn(&EventData) -> bool + Send + Sync + 'static,
        {
            let (shoot, rx) = EventShooter::shoot_out_with(Box::new(selector));
            let mut event_shoots = self.event_shooters.lock().await;
            event_shoots.push(shoot);
            drop(event_shoots);
            let event = rx.await?;
            Ok(event)
        }
    }
    pub fn encode_request(path: &str, echo: u64, r#type: ProcedureCallType) -> String {
        match r#type {
            ProcedureCallType::Request => format!(
                "internal.ProcedureCall\u{0000}|{}\u{0000}|?echo=\u{0000}|{}",
                path, echo
            ),
            ProcedureCallType::Response => format!(
                "internal.ProcedureCall\u{0000}|{}\u{0000}|!echo=\u{0000}|{}",
                path, echo
            ),
        }
    }
    pub fn decode_request(path: &str) -> Result<(String, u64, ProcedureCallType), String> {
        let parts: Vec<&str> = path.split("\u{0000}|").collect();
        if parts.len() != 4 {
            return Err("Invalid request path".to_string());
        }
        let path = parts[1].to_string();
        let echo = parts[3].parse().map_err(|_| "Invalid echo".to_string())?;
        let r#type = match parts[2] {
            "?echo=" => ProcedureCallType::Request,
            "!echo=" => ProcedureCallType::Response,
            _ => return Err("Invalid request path".to_string()),
        };
        Ok((path, echo, r#type))
    }
    #[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
    pub enum ProcedureCallType {
        Request,
        Response,
    }
    #[cfg(feature = "macros")]
    pub use ioevent_macro::{ProcedureCall, procedure};
    pub trait ProcedureCall: Serialize + for<'de> Deserialize<'de> {
        const PATH: &'static str;
    }
    pub trait ProcedureCallRequest:
        ProcedureCall + TryFrom<ProcedureCallData, Error = TryFromEventError> + Sized
    {
        type RESPONSE: ProcedureCallResponse;
        fn upcast(&self, echo: u64) -> Result<ProcedureCallData, CborValueError> {
            Ok(ProcedureCallData {
                path: Self::PATH.to_string(),
                echo,
                r#type: ProcedureCallType::Request,
                data: event::Value::serialized(&self)?,
            })
        }
        fn match_self(other: &ProcedureCallData) -> bool {
            other.path == Self::PATH && other.r#type == ProcedureCallType::Request
        }
    }
    pub trait ProcedureCallResponse:
        ProcedureCall + TryFrom<ProcedureCallData, Error = TryFromEventError> + Sized
    {
        type REQUEST: ProcedureCallRequest;
        fn upcast(&self, echo: u64) -> Result<ProcedureCallData, CborValueError> {
            Ok(ProcedureCallData {
                path: Self::PATH.to_string(),
                echo,
                r#type: ProcedureCallType::Response,
                data: event::Value::serialized(&self)?,
            })
        }
        fn match_echo(other: &ProcedureCallData, echo: u64) -> bool {
            other.path == Self::PATH
                && other.r#type == ProcedureCallType::Response
                && other.echo == echo
        }
    }
    #[derive(Serialize, Deserialize, Clone)]
    pub struct ProcedureCallData {
        pub path: String,
        pub echo: u64,
        pub r#type: ProcedureCallType,
        pub data: event::Value,
    }
    impl From<ProcedureCallData> for EventData {
        fn from(value: ProcedureCallData) -> Self {
            EventData {
                event: encode_request(&value.path, value.echo, value.r#type),
                data: value.data,
            }
        }
    }
    impl Event for ProcedureCallData {
        fn upcast(&self) -> Result<EventData, CborValueError> {
            Ok(self.clone().into())
        }
        const TAG: &'static str = "internal.ProcedureCall";
        const SELECTOR: crate::event::Selector =
            crate::event::Selector(|e| e.event.starts_with(Self::TAG));
    }
    impl TryFrom<&EventData> for ProcedureCallData {
        type Error = TryFromEventError;
        fn try_from(value: &EventData) -> Result<Self, Self::Error> {
            let (path, echo, r#type) = decode_request(&value.event)?;
            Ok(ProcedureCallData {
                path,
                echo,
                r#type,
                data: value.data.clone(),
            })
        }
    }
    pub trait ProcedureCallExt {
        fn call<P>(
            &self,
            procedure: &P,
        ) -> impl Future<Output = Result<P::RESPONSE, CallSubscribeError>>
        where
            P: ProcedureCallRequest;
        fn resolve<P>(
            &self,
            echo: u64,
            response: &P::RESPONSE,
        ) -> impl Future<Output = Result<(), CallSubscribeError>>
        where
            P: ProcedureCallRequest;
    }
    impl<T> ProcedureCallExt for State<T>
    where
        T: ProcedureCallWright,
    {
        async fn call<P>(&self, procedure: &P) -> Result<P::RESPONSE, CallSubscribeError>
        where
            P: ProcedureCallRequest,
        {
            let echo = self.state.next_echo().await;
            let request = procedure.upcast(echo)?;
            self.bus.emit(&request)?;
            let response = self
                .wait_next_with(move |e| {
                    let mut matched = false;
                    if ProcedureCallData::SELECTOR.0(e) {
                        if let Ok(data) = ProcedureCallData::try_from(e) {
                            if P::RESPONSE::match_echo(&data, echo) {
                                matched = true;
                            }
                        }
                    }
                    matched
                })
                .await?;
            let response = ProcedureCallData::try_from(&response)?;
            Ok(P::RESPONSE::try_from(response)?)
        }
        async fn resolve<P>(
            &self,
            echo: u64,
            response: &P::RESPONSE,
        ) -> Result<(), CallSubscribeError>
        where
            P: ProcedureCallRequest,
        {
            let data = response.upcast(echo)?;
            self.bus.emit(&data)?;
            Ok(())
        }
    }
    #[derive(Clone)]
    pub struct DefaultProcedureWright {
        pub rand: Arc<Mutex<SmallRng>>,
    }
    impl Default for DefaultProcedureWright {
        fn default() -> Self {
            Self {
                rand: Arc::new(Mutex::new(SmallRng::from_os_rng())),
            }
        }
    }
    pub trait ProcedureCallWright {
        fn next_echo(&self) -> impl Future<Output = u64> + Send;
    }
    impl ProcedureCallWright for DefaultProcedureWright {
        async fn next_echo(&self) -> u64 {
            let mut rand = self.rand.lock().await;
            rand.next_u64()
        }
    }
}
