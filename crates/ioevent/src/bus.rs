//! A module for managing event communication between different parts of the system.
//!
//! This module provides components for subscribing to events, sending events to external systems,
//! and managing internal event routing. It includes the Bus, SubscribeTicker, EffectTicker,
//! EffectWright, and related components for event-driven communication.

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

/// A ticker that manages event distribution to subscribers.
///
/// Receives events from registered receivers and distributes them to subscribers.
pub struct SubscribeTicker<T: 'static, R> {
    /// Collection of subscribers that receive events
    pub subs: Subscribers<T>,
    /// Collection of event receivers
    pub rx: Vec<channels::Receiver<EventData, R, Cbor>>,
}

impl<T, R> SubscribeTicker<T, R>
where
    T: 'static,
    R: AsyncRead + Unpin,
{
    /// Receives and distributes events to subscribers.
    ///
    /// This method performs the following operations:
    /// 1. Receives an event from any of the registered receivers
    /// 2. Emits the event to all registered subscribers
    /// 3. Processes any event shooters that may be waiting for this event
    ///
    /// # Returns
    /// - `Ok(Iterator<Item = CallSubscribeError>)`: An iterator over any errors that occurred
    ///   while emitting events to subscribers. Empty if all emissions were successful.
    /// - `Err(BusRecvError<R::Error>)`: If all receivers failed to receive an event.
    ///
    /// # Cancel Safety
    /// This method is cancel-safe, meaning it can be safely cancelled at any point without
    /// leaving the system in an inconsistent state.
    pub async fn tick(
        &mut self,
        state: &State<T>,
    ) -> Result<impl Iterator<Item = CallSubscribeError>, BusRecvError<R::Error>> {
        if self.rx.is_empty() {
            return Ok(futures::future::pending().await);
        }
        let iter = self.rx.iter_mut().map(|a| Box::pin(a.recv()));
        let result = future::select_ok(iter).await;
        match result {
            Ok((e, v)) => {
                let results = task::unconstrained(async {
                    drop(v);
                    let results = self.subs.emit(state, &e).await;
                    let len = state.event_shooters.len();
                    for _ in 0..len {
                        if let Some(st) = state.event_shooters.pop() {
                            if let Some(st) = st.try_shoot_out(&e) {
                                state.event_shooters.push(st);
                            }
                        }
                    }
                    results
                })
                .await;
                Ok(results)
            }
            Err(e) => Err(e.into()),
        }
    }
}

/// A ticker that manages event emission to external systems.
///
/// Receives events from the internal state channel and forwards them to registered senders.
pub struct EffectTicker<W> {
    /// Collection of event senders
    pub tx: Vec<channels::Sender<EventData, W, Cbor>>,
    /// Receiver for events from the internal state channel
    pub state_rx: tokio::sync::mpsc::UnboundedReceiver<EventData>,
}

impl<W> EffectTicker<W>
where
    W: AsyncWrite + Unpin,
{
    /// Receives an event from the state channel and sends it to all registered senders.
    ///
    /// This method performs the following operations:
    /// 1. Receives an event from the state channel
    /// 2. Sends the event to all registered senders in parallel
    /// 3. Collects and returns any errors that occurred during sending
    ///
    /// # Returns
    /// An iterator over any errors that occurred while sending events.
    /// The iterator will be empty if all sends were successful.
    ///
    /// # Cancel Safety
    /// This method is cancel-safe, meaning it can be safely cancelled at any point without
    /// leaving the system in an inconsistent state.
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

/// A component responsible for emitting events to the effect channel.
///
/// This struct provides a simple interface for sending events to the internal
/// state channel, which will then be processed by the EffectTicker.
#[derive(Clone)]
pub struct EffectWright {
    /// Sender for the internal state channel
    pub state_tx: tokio::sync::mpsc::UnboundedSender<EventData>,
}

impl EffectWright {
    /// Emits an event to the effect channel.
    ///
    /// This method performs the following operations:
    /// 1. Converts the event to EventData using upcast
    /// 2. Sends the event to the internal state channel
    ///
    /// # Arguments
    /// * `event` - The event to emit
    ///
    /// # Returns
    /// * `Ok(())` - If the event was successfully sent
    /// * `Err(CallSubscribeError)` - If there was an error during conversion or sending
    pub fn emit<E>(&self, event: &E) -> Result<(), CallSubscribeError>
    where
        E: Event,
    {
        let event = event.upcast()?;
        self.state_tx.send(event)?;
        Ok(())
    }
}

/// Central component for event communication in the system.
///
/// Manages event flow between:
/// * External sources (via SubscribeTicker)
/// * External destinations (via EffectTicker)
/// * Internal state (via EffectWright)
pub struct Bus<T, W, R>
where
    T: 'static,
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
{
    /// Component for receiving and distributing events
    pub subscribe_ticker: SubscribeTicker<T, R>,
    /// Component for sending events to external systems
    pub effect_ticker: EffectTicker<W>,
    /// Component for emitting events to internal state
    pub effect_wright: EffectWright,
}

/// A pair of I/O components for bidirectional event communication.
pub struct IoPair<IR, IW> {
    /// Reader component for receiving events
    pub reader: IR,
    /// Writer component for sending events
    pub writer: IW,
}

impl IoPair<tokio::io::Stdin, tokio::io::Stdout> {
    /// Creates a new I/O pair using standard input and output streams.
    ///
    /// This is particularly useful for command-line applications that need to
    /// communicate with their parent process.
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

/// A builder for creating and configuring a Bus instance.
///
/// This builder provides a fluent interface for setting up all the components
/// needed for a fully functional Bus. It allows for incremental configuration
/// of readers, writers, and subscribers.
pub struct BusBuilder<T, W, R>
where
    T: 'static,
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
{
    /// Collection of subscribers that will receive events
    subs: Subscribers<T>,
    /// Collection of event receivers
    rx: Vec<channels::Receiver<EventData, R, Cbor>>,
    /// Collection of event senders
    tx: Vec<channels::Sender<EventData, W, Cbor>>,
    /// Receiver for the internal state channel
    state_rx: tokio::sync::mpsc::UnboundedReceiver<EventData>,
    /// Sender for the internal state channel
    state_tx: tokio::sync::mpsc::UnboundedSender<EventData>,
}

impl<T, W, R> BusBuilder<T, W, R>
where
    T: 'static,
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
{
    /// Creates a new BusBuilder with the specified subscribers.
    ///
    /// # Arguments
    /// * `subscribes` - The collection of subscribers that will receive events
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
    /// Adds a reader to the bus configuration.
    ///
    /// # Arguments
    /// * `reader` - The reader to add, must implement IntoRead<R>
    ///
    /// # Returns
    /// The builder instance for method chaining
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
    /// Adds a writer to the bus configuration.
    ///
    /// # Arguments
    /// * `writer` - The writer to add, must implement IntoWrite<W>
    ///
    /// # Returns
    /// The builder instance for method chaining
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
    /// Adds a reader-writer pair to the bus configuration.
    ///
    /// # Arguments
    /// * `pair` - The I/O pair to add
    ///
    /// # Returns
    /// The builder instance for method chaining
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
    /// Builds and returns a configured Bus instance.
    ///
    /// # Returns
    /// A fully configured Bus instance ready for use
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
    use std::{collections::HashMap, hash::Hash, ops::Deref, sync::Arc};

    use crossbeam_queue::SegQueue;
    use rand::{RngCore, SeedableRng, rngs::SmallRng};
    use serde::{Deserialize, Serialize};
    use tokio::sync::{Mutex, oneshot};

    use crate::{
        error::{CallSubscribeError, CborValueError, TryFromEventError},
        event::{self, Event, EventData},
    };

    use super::EffectWright;

    #[cfg(feature = "macros")]
    pub use ioevent_macro::{ProcedureCall, procedure};

    /// A component that can selectively emit events to a receiver.
    ///
    /// This struct combines a selector function that determines which events to emit
    /// with a oneshot channel for sending the selected events.
    pub struct EventShooter {
        /// Function that determines whether an event should be emitted
        pub selector: Box<dyn Fn(&EventData) -> bool + Send + Sync + 'static>,
        /// Channel for sending selected events
        pub shooter: oneshot::Sender<EventData>,
    }

    impl EventShooter {
        /// Creates a new EventShooter with the specified selector function.
        ///
        /// # Arguments
        /// * `selector` - A function that determines which events should be emitted
        ///
        /// # Returns
        /// A tuple containing:
        /// * The EventShooter instance
        /// * A oneshot receiver that will receive the selected events
        pub fn shoot_out_with(
            selector: Box<dyn Fn(&EventData) -> bool + Send + Sync + 'static>,
        ) -> (Self, oneshot::Receiver<EventData>) {
            let (shooter, receiver) = oneshot::channel();
            (Self { selector, shooter }, receiver)
        }
        /// Attempts to emit an event if it matches the selector.
        ///
        /// # Arguments
        /// * `event` - The event to potentially emit
        ///
        /// # Returns
        /// * `None` - If the event was emitted
        /// * `Some(self)` - If the event was not emitted (the selector didn't match)
        pub fn try_shoot_out(self, event: &EventData) -> Option<Self> {
            if (self.selector)(event) {
                unsafe { self.shoot_out(event) };
                None
            } else {
                Some(self)
            }
        }
        /// Emits an event through the oneshot channel.
        ///
        /// # Safety
        /// This method is marked unsafe because it bypasses the selector check.
        /// The caller must ensure that the event is appropriate for emission.
        ///
        /// # Arguments
        /// * `event` - The event to emit
        ///
        /// # Returns
        /// `true` if the event was successfully sent, `false` otherwise
        pub unsafe fn shoot_out(self, event: &EventData) -> bool {
            self.shooter.send(event.clone()).is_ok()
        }
    }
    /// The state of the bus, used for collecting and managing side effects.
    ///
    /// This struct maintains the current state of the system along with components
    /// for managing event emission and event shooters.
    #[derive(Clone)]
    pub struct State<T> {
        /// The current state value
        pub state: T,
        /// Component for emitting events to the bus
        pub bus: EffectWright,
        /// Queue of event shooters waiting for specific events
        pub event_shooters: Arc<SegQueue<EventShooter>>,
    }

    impl<T> Deref for State<T> {
        type Target = T;
        fn deref(&self) -> &Self::Target {
            &self.state
        }
    }

    impl<T> State<T> {
        /// Creates a new State instance with the specified state value and bus.
        ///
        /// # Arguments
        /// * `state` - The initial state value
        /// * `bus` - The EffectWright instance for emitting events
        pub fn new(state: T, bus: EffectWright) -> Self {
            Self {
                state,
                bus,
                event_shooters: Arc::new(SegQueue::new()),
            }
        }
        /// Waits for the next event of a specific type.
        ///
        /// # Arguments
        /// * `E` - The type of event to wait for
        ///
        /// # Returns
        /// * `Ok(E)` - The received event
        /// * `Err(CallSubscribeError)` - If there was an error receiving or converting the event
        pub async fn wait_next<E>(&self) -> Result<E, CallSubscribeError>
        where
            E: Event,
        {
            let event = self.wait_next_with(E::SELECTOR.0).await?;
            Ok(E::try_from(&event)?)
        }
        /// Waits for the next event that matches the specified selector function.
        ///
        /// # Arguments
        /// * `selector` - A function that determines which events to accept
        ///
        /// # Returns
        /// * `Ok(EventData)` - The received event data
        /// * `Err(CallSubscribeError)` - If there was an error receiving the event
        pub async fn wait_next_with<F>(&self, selector: F) -> Result<EventData, CallSubscribeError>
        where
            F: Fn(&EventData) -> bool + Send + Sync + 'static,
        {
            let (shoot, rx) = EventShooter::shoot_out_with(Box::new(selector));
            self.event_shooters.push(shoot);
            let event = rx.await?;
            Ok(event)
        }
    }
    /// Encodes a procedure call request or response into a string format.
    ///
    /// # Arguments
    /// * `path` - The path of the procedure
    /// * `echo` - The echo identifier
    /// * `type` - The type of procedure call (Request or Response)
    ///
    /// # Returns
    /// A formatted string representing the procedure call
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
    /// Decodes a procedure call string into its components.
    ///
    /// # Arguments
    /// * `path` - The encoded procedure call string
    ///
    /// # Returns
    /// * `Ok((String, u64, ProcedureCallType))` - The decoded components (path, echo, type)
    /// * `Err(String)` - If the string is not a valid procedure call
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
    /// The type of a procedure call (Request or Response).
    #[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
    pub enum ProcedureCallType {
        /// A request to execute a procedure
        Request,
        /// A response from a procedure execution
        Response,
    }
    /// A trait for types that can be used in procedure calls.
    ///
    /// This trait provides the basic functionality needed for serialization
    /// and deserialization of procedure calls.
    pub trait ProcedureCall: Serialize + for<'de> Deserialize<'de> {
        /// The path identifier for this procedure call
        fn path() -> String;
    }
    /// A trait for procedure call requests.
    ///
    /// This trait extends ProcedureCall with functionality specific to requests,
    /// including the ability to upcast to ProcedureCallData and match against
    /// other procedure calls.
    pub trait ProcedureCallRequest:
        ProcedureCall + TryFrom<ProcedureCallData, Error = TryFromEventError> + Sized
    {
        /// The associated response type for this request
        type RESPONSE: ProcedureCallResponse;

        /// Converts this request into a ProcedureCallData with the given echo.
        ///
        /// # Arguments
        /// * `echo` - The echo identifier to use
        ///
        /// # Returns
        /// * `Ok(ProcedureCallData)` - The converted procedure call data
        /// * `Err(CborValueError)` - If serialization fails
        fn upcast(&self, echo: u64) -> Result<ProcedureCallData, CborValueError> {
            Ok(ProcedureCallData {
                path: Self::path(),
                echo,
                r#type: ProcedureCallType::Request,
                data: event::Value::serialized(&self)?,
            })
        }
        /// Checks if another procedure call matches this request.
        ///
        /// # Arguments
        /// * `other` - The procedure call to check against
        ///
        /// # Returns
        /// `true` if the procedure calls match, `false` otherwise
        fn match_self(other: &ProcedureCallData) -> bool {
            other.path == Self::path() && other.r#type == ProcedureCallType::Request
        }
    }
    /// A trait for procedure call responses.
    ///
    /// This trait extends ProcedureCall with functionality specific to responses,
    /// including the ability to upcast to ProcedureCallData and match against
    /// requests using echo identifiers.
    pub trait ProcedureCallResponse:
        ProcedureCall + TryFrom<ProcedureCallData, Error = TryFromEventError> + Sized
    {
        /// Converts this response into a ProcedureCallData with the given echo.
        ///
        /// # Arguments
        /// * `echo` - The echo identifier to use
        ///
        /// # Returns
        /// * `Ok(ProcedureCallData)` - The converted procedure call data
        /// * `Err(CborValueError)` - If serialization fails
        fn upcast(&self, echo: u64) -> Result<ProcedureCallData, CborValueError> {
            Ok(ProcedureCallData {
                path: Self::path(),
                echo,
                r#type: ProcedureCallType::Response,
                data: event::Value::serialized(&self)?,
            })
        }
        fn match_echo(other: &ProcedureCallData, echo: u64) -> bool {
            other.path == Self::path()
                && other.r#type == ProcedureCallType::Response
                && other.echo == echo
        }
    }
    /// The data structure representing a procedure call.
    ///
    /// This struct contains all the information needed to represent either a
    /// request or response in a procedure call system. It includes the path
    /// identifier, echo value for matching requests and responses, the type
    /// of call (request or response), and the serialized data payload.
    #[derive(Serialize, Deserialize, Clone)]
    pub struct ProcedureCallData {
        /// The path identifier for the procedure being called
        pub path: String,
        /// A unique identifier used to match requests with their corresponding responses
        pub echo: u64,
        /// The type of procedure call (Request or Response)
        pub r#type: ProcedureCallType,
        /// The serialized data payload of the procedure call
        pub data: event::Value,
    }

    impl From<ProcedureCallData> for EventData {
        /// Converts a ProcedureCallData into an EventData.
        ///
        /// This implementation:
        /// 1. Encodes the procedure call information into the event string
        /// 2. Preserves the data payload
        ///
        /// # Arguments
        /// * `value` - The ProcedureCallData to convert
        ///
        /// # Returns
        /// An EventData instance containing the encoded procedure call
        fn from(value: ProcedureCallData) -> Self {
            EventData {
                event: encode_request(&value.path, value.echo, value.r#type),
                data: value.data,
            }
        }
    }

    impl Event for ProcedureCallData {
        /// Converts this procedure call into an EventData.
        ///
        /// This implementation:
        /// 1. Clones the current instance
        /// 2. Converts it to EventData using the From implementation
        ///
        /// # Returns
        /// * `Ok(EventData)` - The converted event data
        /// * `Err(CborValueError)` - If serialization fails
        fn upcast(&self) -> Result<EventData, CborValueError> {
            Ok(self.clone().into())
        }

        /// The tag used to identify procedure call events in the event system
        const TAG: &'static str = "internal.ProcedureCall";

        /// The selector used to identify procedure call events
        const SELECTOR: crate::event::Selector =
            crate::event::Selector(|e| e.event.starts_with(Self::TAG));
    }

    impl TryFrom<&EventData> for ProcedureCallData {
        type Error = TryFromEventError;

        /// Attempts to convert an EventData into a ProcedureCallData.
        ///
        /// This implementation:
        /// 1. Decodes the procedure call information from the event string
        /// 2. Preserves the data payload
        ///
        /// # Arguments
        /// * `value` - The event data to convert
        ///
        /// # Returns
        /// * `Ok(ProcedureCallData)` - The converted procedure call data
        /// * `Err(TryFromEventError)` - If the conversion fails
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
    /// A trait providing extension methods for procedure calls.
    ///
    /// This trait adds convenient methods for making procedure calls and
    /// handling responses.
    pub trait ProcedureCallExt {
        /// Makes a procedure call and waits for the response.
        ///
        /// # Arguments
        /// * `procedure` - The procedure request to execute
        ///
        /// # Returns
        /// A future that resolves to the procedure response or an error
        fn call<P>(
            &self,
            procedure: &P,
        ) -> impl Future<Output = Result<P::RESPONSE, CallSubscribeError>>
        where
            P: ProcedureCallRequest;

        /// Resolves a procedure call with a response.
        ///
        /// # Arguments
        /// * `echo` - The echo identifier of the original request
        /// * `response` - The response to send
        ///
        /// # Returns
        /// A future that resolves when the response is sent or an error occurs
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
        /// Makes a procedure call and waits for the response.
        ///
        /// This implementation:
        /// 1. Generates a new echo identifier
        /// 2. Sends the request
        /// 3. Waits for a matching response
        /// 4. Returns the response or an error
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
        /// Resolves a procedure call with a response.
        ///
        /// This implementation:
        /// 1. Converts the response to ProcedureCallData
        /// 2. Sends it through the bus
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
    /// A default implementation of ProcedureCallWright that uses a random number generator.
    ///
    /// This struct provides a thread-safe way to generate unique echo identifiers
    /// for procedure calls.
    #[derive(Clone)]
    pub struct DefaultProcedureWright {
        /// A thread-safe random number generator
        pub rand: Arc<Mutex<SmallRng>>,
    }

    impl Default for DefaultProcedureWright {
        /// Creates a new DefaultProcedureWright with a random seed.
        fn default() -> Self {
            Self {
                rand: Arc::new(Mutex::new(SmallRng::from_os_rng())),
            }
        }
    }
    /// A trait for generating unique echo identifiers for procedure calls.
    pub trait ProcedureCallWright {
        /// Generates the next echo identifier.
        ///
        /// # Returns
        /// A future that resolves to a unique echo identifier
        fn next_echo(&self) -> impl Future<Output = u64> + Send;
    }

    impl ProcedureCallWright for DefaultProcedureWright {
        /// Generates the next echo identifier using the random number generator.
        ///
        /// This implementation:
        /// 1. Locks the random number generator
        /// 2. Generates a random u64
        /// 3. Returns it as the echo identifier
        async fn next_echo(&self) -> u64 {
            let mut rand = self.rand.lock().await;
            rand.next_u64()
        }
    }

    /* Start Default Implementation of ProcedureCallResponse */
    impl ProcedureCall for () {
        fn path() -> String {
            "core::Unit".to_owned()
        }
    }
    impl ProcedureCallResponse for () {}
    impl TryFrom<ProcedureCallData> for () {
        type Error = TryFromEventError;
        fn try_from(_: ProcedureCallData) -> Result<Self, Self::Error> {
            Ok(())
        }
    }

    impl<T, E> ProcedureCall for Result<T, E>
    where
        T: ProcedureCall,
        E: ProcedureCall,
    {
        fn path() -> String {
            format!("core::Result<{}, {}>", T::path(), E::path())
        }
    }
    impl<T, E> ProcedureCallResponse for Result<T, E>
    where
        T: ProcedureCallResponse,
        E: ProcedureCallResponse,
    {
    }
    impl<T, E> TryFrom<ProcedureCallData> for Result<T, E>
    where
        T: ProcedureCall,
        E: ProcedureCall,
    {
        type Error = TryFromEventError;
        fn try_from(value: ProcedureCallData) -> Result<Self, Self::Error> {
            Ok(value.data.deserialized()?)
        }
    }

    impl<T> ProcedureCall for Option<T>
    where
        T: ProcedureCall,
    {
        fn path() -> String {
            format!("core::Option<{}>", T::path())
        }
    }
    impl<T> ProcedureCallResponse for Option<T> where T: ProcedureCallResponse {}
    impl<T> TryFrom<ProcedureCallData> for Option<T>
    where
        T: ProcedureCall,
    {
        type Error = TryFromEventError;
        fn try_from(value: ProcedureCallData) -> Result<Self, Self::Error> {
            Ok(value.data.deserialized()?)
        }
    }

    impl<T> ProcedureCall for Vec<T>
    where
        T: ProcedureCall,
    {
        fn path() -> String {
            format!("core::Vec<{}>", T::path())
        }
    }
    impl<T> ProcedureCallResponse for Vec<T> where T: ProcedureCallResponse {}
    impl<T> TryFrom<ProcedureCallData> for Vec<T>
    where
        T: ProcedureCall,
    {
        type Error = TryFromEventError;
        fn try_from(value: ProcedureCallData) -> Result<Self, Self::Error> {
            Ok(value.data.deserialized()?)
        }
    }

    impl<K, V> ProcedureCall for HashMap<K, V>
    where
        K: ProcedureCall + Hash + Eq,
        V: ProcedureCall,
    {
        fn path() -> String {
            format!("core::HashMap<{}, {}>", K::path(), V::path())
        }
    }
    impl<K, V> ProcedureCallResponse for HashMap<K, V>
    where
        K: ProcedureCallResponse + Hash + Eq,
        V: ProcedureCallResponse,
    {
    }
    impl<K, V> TryFrom<ProcedureCallData> for HashMap<K, V>
    where
        K: ProcedureCallResponse + Hash + Eq,
        V: ProcedureCallResponse,
    {
        type Error = TryFromEventError;
        fn try_from(value: ProcedureCallData) -> Result<Self, Self::Error> {
            Ok(value.data.deserialized()?)
        }
    }

    macro_rules! impl_procedure_call {
        ($($t:ty),*) => {
            $(
                impl ProcedureCall for $t {
                    fn path() -> String {
                        concat!("core::", stringify!($t)).to_owned()
                    }
                }
                impl ProcedureCallResponse for $t {}
                impl TryFrom<ProcedureCallData> for $t {
                    type Error = TryFromEventError;
                    fn try_from(value: ProcedureCallData) -> Result<Self, Self::Error> {
                        Ok(value.data.deserialized()?)
                    }
                }
            )*
        };
    }

    impl_procedure_call!(
        String, bool, u8, u16, u32, u64, i8, i16, i32, i64, f32, f64, char
    );

    macro_rules! impl_procedure_call_tuple {
        ($($t:ident),*) => {
            impl<$($t: ProcedureCall),*> ProcedureCall for ($($t,)*) {
                fn path() -> String {
                    "core::Tuple".to_owned() + "(" + $($t::path().as_str() + ", " +)* ")"
                }
            }
            impl<$($t: ProcedureCallResponse),*> ProcedureCallResponse for ($($t,)*) {}
            impl<$($t: ProcedureCall),*> TryFrom<ProcedureCallData> for ($($t,)*) {
                type Error = TryFromEventError;
                fn try_from(value: ProcedureCallData) -> Result<Self, Self::Error> {
                    Ok(value.data.deserialized()?)
                }
            }
        };
    }

    impl_procedure_call_tuple!(P0);
    impl_procedure_call_tuple!(P0, P1);
    impl_procedure_call_tuple!(P0, P1, P2);
    impl_procedure_call_tuple!(P0, P1, P2, P3);
    impl_procedure_call_tuple!(P0, P1, P2, P3, P4);
    impl_procedure_call_tuple!(P0, P1, P2, P3, P4, P5);
    impl_procedure_call_tuple!(P0, P1, P2, P3, P4, P5, P6);
    impl_procedure_call_tuple!(P0, P1, P2, P3, P4, P5, P6, P7);
    impl_procedure_call_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8);
    impl_procedure_call_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9);
    impl_procedure_call_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10);
    impl_procedure_call_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11);
    impl_procedure_call_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12);
    impl_procedure_call_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13);
    impl_procedure_call_tuple!(
        P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13, P14
    );
    impl_procedure_call_tuple!(
        P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13, P14, P15
    );
}
