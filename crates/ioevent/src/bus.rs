//! Event bus module for managing event communication between different parts of the system.
//!
//! This module provides the core components for event-driven communication:
//! - Event routing and distribution
//! - Event subscription management
//! - External system integration
//! - Event emission to external systems
//!
//! # Architecture Overview
//! The event bus system consists of several key components:
//! - [`Bus`]: The central event router
//! - [`SubscribeTicker`]: Manages event distribution to subscribers
//! - [`EffectTicker`]: Handles event emission to external systems
//! - [`EffectWright`]: Provides a simple interface for event emission
//! - Uses [`State`] from the state module for managing application state and event shooters
//!
//! # Examples
//! ```rust
//! use ioevent::prelude::*;
//!
//! // Create a new bus with subscribers
//! let (_, bus) = BusBuilder::new(subscribers)
//!     .add_reader(stdin)
//!     .add_writer(stdout)
//!     .build();
//!
//! // Run the bus with error handling
//! bus.run(state, &handle_error).await;
//! ```
//!
//! For more detailed examples and usage patterns, see the individual component documentation.

use std::{
    iter, mem,
    pin::Pin,
    task::{Context, Poll},
};

use crate::state::State;
use channels::{
    io::{AsyncRead, AsyncWrite, IntoRead, IntoWrite},
    serdes::Cbor,
};
use crossbeam_queue::{ArrayQueue, SegQueue};
use futures::future::{self, join_all, pending};
use tokio::{
    select,
    sync::broadcast,
    task::{self, JoinHandle},
};
use tokio_util::sync::CancellationToken;
use triomphe::Arc;

use crate::{
    error::{BusError, BusSendError, CallSubscribeError},
    event::*,
    util::CenterErrorIter,
};

/// A ticker that manages event distribution between multiple channels.
///
/// This component is responsible for:
/// - Receiving events from multiple input channels
/// - Distributing events to registered output channels
/// - Managing channel lifecycle and error handling
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// let mut center_ticker = CenterTicker::new(receivers);
/// let receiver = center_ticker.new_receiver();
/// ```
pub struct CenterTicker<R> {
    /// Collection of event receivers
    pub rx: Vec<channels::Receiver<EventData, R, Cbor>>,
    /// Collection of event senders
    pub tx: Vec<tokio::sync::mpsc::UnboundedSender<EventData>>,
}

impl<R> CenterTicker<R>
where
    R: AsyncRead + Unpin,
{
    /// Creates a new CenterTicker with the specified receivers.
    ///
    /// # Arguments
    /// * `rx` - A collection of event receivers
    pub fn new(rx: Vec<channels::Receiver<EventData, R, Cbor>>) -> Self {
        Self { rx, tx: Vec::new() }
    }

    /// Processes events from all receivers and distributes them to all senders.
    ///
    /// This method performs the following operations:
    /// 1. Waits for an event from any of the registered receivers
    /// 2. Sends the event to all registered senders in parallel
    /// 3. Collects and returns any errors that occurred during sending
    ///
    /// # Returns
    /// A CenterErrorIter that yields:
    /// * Left variant: Errors from sending events through channels
    /// * Right variant: Errors from receiving events from channels
    ///
    /// # Cancel Safety
    /// This method is cancel-safe, meaning it can be safely cancelled at any point without
    /// leaving the system in an inconsistent state.
    pub async fn tick(
        &mut self,
    ) -> CenterErrorIter<impl Iterator<Item = tokio::sync::mpsc::error::SendError<EventData>>, R>
    {
        if self.rx.is_empty() {
            pending::<()>().await;
        }
        let iter = self.rx.iter_mut().map(|a| Box::pin(a.recv()));
        let result = future::select_ok(iter).await;
        match result {
            Ok((e, v)) => {
                let results = task::unconstrained(async {
                    drop(v);
                    let results = self.tx.iter_mut().map(move |a| a.send(e.clone()));
                    results.into_iter().filter_map(Result::err)
                })
                .await;
                CenterErrorIter::Left(results)
            }
            Err(e) => CenterErrorIter::Right(Some(e.into())),
        }
    }

    /// Creates a new receiver for events from this ticker.
    ///
    /// This method creates a new channel and adds its sender to the internal
    /// collection of senders. The returned receiver can be used to receive
    /// events that are processed by this ticker.
    ///
    /// # Returns
    /// A new receiver for events from this ticker
    pub fn new_receiver(&mut self) -> tokio::sync::mpsc::UnboundedReceiver<EventData> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.tx.push(tx);
        rx
    }
}

/// A ticker that manages event distribution to subscribers.
///
/// This component is responsible for:
/// - Receiving events from registered channels
/// - Distributing events to matching subscribers
/// - Managing subscriber lifecycle and error handling
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// let mut subscribe_ticker = SubscribeTicker {
///     subs: subscribers,
///     rx: receiver,
/// };
///
/// subscribe_ticker.tick(&state).await;
/// ```
pub struct SubscribeTicker<T: 'static> {
    /// Collection of subscribers that receive events
    pub subs: Subscribers<T>,
    /// Collection of event receivers
    pub rx: tokio::sync::mpsc::UnboundedReceiver<EventData>,
    /// Queue of errors that occurred during event distribution
    pub err_queue: Arc<SegQueue<CallSubscribeError>>,
}

impl<T> SubscribeTicker<T>
where
    T: Clone + Send + Sync + 'static,
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
    /// This method is NOT! cancel-safe.
    pub async fn tick(
        &mut self,
        state: &State<T>,
    ) -> impl Iterator<Item = CallSubscribeError> + Send + 'static {
        let event = self.rx.recv().await;
        if let Some(event) = event {
            let results = self.subs.emit(state, &event).await;
            Some(results)
        } else {
            None
        }
        .into_iter()
        .flatten()
    }

    /// Attempts to process events without blocking.
    ///
    /// This method performs the following operations:
    /// 1. Attempts to receive an event from the receiver without blocking
    /// 2. If an event is received, spawns a task to process it asynchronously
    /// 3. Collects any errors that occurred during processing
    ///
    /// # Arguments
    /// * `state` - The current state
    ///
    /// # Returns
    /// An iterator over any errors that occurred during event processing.
    /// The iterator will be empty if no event was received or if all processing
    /// was successful.
    ///
    /// # Cancel Safety
    /// This method is cancel-safe, meaning it can be safely cancelled at any point without
    /// leaving the system in an inconsistent state.
    pub async fn try_tick(
        &mut self,
        state: &State<T>,
    ) -> impl Iterator<Item = CallSubscribeError> + Send + 'static {
        let event = self.rx.try_recv();
        if let Ok(event) = event {
            let subs = self.subs.clone();
            let state = state.clone();
            let err_queue = self.err_queue.clone();
            tokio::spawn(UnsafeSendFuture(async move {
                let results = subs.emit(&state, &event).await;
                for result in results {
                    err_queue.push(result);
                }
            }));
        }
        let queue = ArrayQueue::new(self.err_queue.len());
        while let Some(err) = self.err_queue.pop() {
            let _ = queue.push(err);
            if queue.is_full() {
                break;
            }
        }
        queue.into_iter()
    }
}

/// A ticker that manages event distribution to event shooters.
///
/// This component is responsible for:
/// - Receiving events from the event bus
/// - Distributing events to matching event shooters
/// - Managing event shooter lifecycle
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// let mut shooter_ticker = ShooterTicker {
///     rx: receiver,
/// };
///
/// shooter_ticker.tick(&state).await;
/// ```
pub struct ShooterTicker {
    /// Receiver for events from the event bus
    pub rx: tokio::sync::mpsc::UnboundedReceiver<EventData>,
}

impl ShooterTicker {
    /// Processes events and distributes them to matching event shooters.
    ///
    /// This method performs the following operations:
    /// 1. Receives an event from the event bus
    /// 2. Attempts to dispatch the event to all registered event shooters
    /// 3. Maintains shooters that did not match the event for future processing
    ///
    /// # Arguments
    /// * `state` - The current state containing the event shooters
    ///
    /// # Cancel Safety
    /// This method is cancel-safe, meaning it can be safely cancelled at any point without
    /// leaving the system in an inconsistent state.
    pub async fn tick<T>(&mut self, state: &State<T>) {
        let event = self.rx.recv().await;
        if let Some(event) = event {
            task::unconstrained(async {
                let mut beginning = state.event_shooters.lock().await;
                let mut then = Vec::with_capacity(beginning.len());
                mem::swap(&mut *beginning, &mut then);
                for shooter in then.into_iter() {
                    if let Some(shooter) = shooter.try_dispatch(&event) {
                        beginning.push(shooter);
                    }
                }
            })
            .await;
        }
    }
}

/// A ticker that manages event emission to external systems.
///
/// This component is responsible for:
/// - Receiving events from the internal state channel
/// - Forwarding events to registered external systems
/// - Managing connection lifecycle and error handling
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// let mut effect_ticker = EffectTicker {
///     tx: senders,
///     state_rx: receiver,
/// };
///
/// effect_ticker.tick().await;
/// ```
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
/// The `Bus` is the main component that coordinates all event communication:
/// - Routes events between different parts of the system
/// - Manages event distribution to subscribers
/// - Handles event emission to external systems
/// - Coordinates state management and event shooters
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// let bus = Bus {
///     center_ticker,
///     subscribe_ticker,
///     effect_ticker,
///     sooter_ticker,
/// };
///
/// bus.run(state, &handle_error).await;
/// ```
pub struct Bus<T, W, R>
where
    T: 'static + Send + Sync,
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
{
    /// Component for receiving and distributing events
    pub center_ticker: CenterTicker<R>,
    /// Component for receiving and distributing events
    pub subscribe_ticker: SubscribeTicker<T>,
    /// Component for sending events to external systems
    pub effect_ticker: EffectTicker<W>,
    /// Component for receiving events from external systems
    pub shooter_ticker: ShooterTicker,
}
impl<T, W, R> Bus<T, W, R>
where
    T: Clone + Send + Sync + 'static,
    W: AsyncWrite + Unpin + 'static,
    R: AsyncRead + Unpin + 'static,
{
    /// Runs the event bus with the specified state and error handler.
    ///
    /// This method performs the following operations:
    /// 1. Initializes all components of the event bus
    /// 2. Spawns tasks for processing events in parallel
    /// 3. Handles errors using the provided error handler
    /// 4. Provides a way to gracefully shut down the bus
    ///
    /// # Arguments
    /// * `state` - The initial state for the event bus
    /// * `handle_error` - A function that handles any errors that occur during event processing
    ///
    /// # Returns
    /// A CloseHandle that can be used to:
    /// * Gracefully shut down the bus
    /// * Wait for the bus to complete
    /// * Spawn the bus in a new task
    ///
    /// # Cancel Safety
    /// This method is cancel-safe, meaning it can be safely cancelled at any point without
    /// leaving the system in an inconsistent state.
    pub async fn run<F>(
        self,
        state: State<T>,
        handle_error: &'static F,
    ) -> CloseHandle<impl Future<Output = ()>>
    where
        F: Fn(BusError<W::Error, R::Error>) + Send + Sync + 'static,
    {
        let token = CancellationToken::new();
        let (close_signal, mut close_signal_receiver) = broadcast::channel::<()>(1);
        let Bus {
            mut center_ticker,
            mut subscribe_ticker,
            mut effect_ticker,
            mut shooter_ticker,
            ..
        } = self;
        let state_clone = state.clone();
        let token_clone = token.clone();
        let handle_subscribe_ticker = tokio::spawn(UnsafeSendFuture(async move {
            loop {
                if token_clone.is_cancelled() {
                    break;
                }
                let error = subscribe_ticker.tick(&state_clone).await;
                error.map(|e| e.into()).for_each(handle_error);
            }
        }));

        let state_clone = state.clone();
        let token_clone = token.clone();
        let handle_shooter_ticker = tokio::spawn(async move {
            loop {
                if token_clone.is_cancelled() {
                    break;
                }
                shooter_ticker.tick(&state_clone).await;
            }
        });
        let future = async move {
            loop {
                select! {
                    errors = effect_ticker.tick() => {
                        errors.map(|e| e.into()).for_each(handle_error);
                    }
                    errors = center_ticker.tick() => {
                        errors.map(|e| e.into()).for_each(handle_error);
                    }
                    _ = close_signal_receiver.recv() => {
                        token.cancel();
                        handle_subscribe_ticker.abort();
                        handle_shooter_ticker.abort();
                        break;
                    }
                }
            }
        };
        CloseHandle {
            close_signal: CloseSignal(close_signal),
            future,
        }
    }
}

/// A signal for closing the event bus.
///
/// This struct provides a way to send a close signal to the event bus,
/// which will gracefully shut down all components.
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// let close_signal = CloseSignal(sender);
/// close_signal.close();
/// ```
pub struct CloseSignal(broadcast::Sender<()>);

impl CloseSignal {
    /// Sends a close signal to the event bus.
    ///
    /// This method sends a signal to all components of the event bus,
    /// instructing them to shut down gracefully.
    pub fn close(self) {
        self.0.send(()).unwrap();
    }
}

/// A handle for managing the lifecycle of the event bus.
///
/// This struct combines a close signal with a future that represents
/// the running event bus. It provides methods for gracefully shutting
/// down the bus and waiting for it to complete.
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// let handle = CloseHandle {
///     close_signal,
///     future,
/// };
///
/// // Gracefully shut down the bus
/// handle.close().await;
/// ```
pub struct CloseHandle<F>
where
    F: Future<Output = ()>,
{
    /// Signal for closing the event bus
    pub close_signal: CloseSignal,
    /// Future representing the running event bus
    pub future: F,
}

impl<F> CloseHandle<F>
where
    F: Future<Output = ()> + Send + 'static,
{
    /// Gracefully shuts down the event bus and waits for it to complete.
    ///
    /// This method:
    /// 1. Sends a close signal to all components
    /// 2. Waits for the event bus to complete its shutdown
    pub async fn close(self) {
        self.close_signal.close();
        self.future.await;
    }

    /// Waits for the event bus to complete without sending a close signal.
    ///
    /// This method is useful when you want to wait for the bus to complete
    /// naturally, without forcing a shutdown.
    pub async fn join(self) {
        self.future.await;
    }

    /// Spawns the event bus in a new task and returns a handle to it.
    ///
    /// This method:
    /// 1. Spawns the event bus future in a new task
    /// 2. Returns a tuple containing:
    ///    - A handle to the spawned task
    ///    - The close signal for shutting down the bus
    pub fn spawn(self) -> (JoinHandle<()>, CloseSignal) {
        (tokio::spawn(self.future), self.close_signal)
    }
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
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// let mut builder = BusBuilder::new(subscribers);
/// builder.add_reader(stdin);
/// builder.add_writer(stdout);
/// let (bus, effect_wright) = builder.build();
/// ```
pub struct BusBuilder<T, W, R>
where
    T: 'static + Send + Sync,
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
    T: 'static + Send + Sync,
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
    /// A tuple containing:
    /// * A fully configured Bus instance ready for use
    /// * An EffectWright instance for emitting events
    pub fn build(self) -> (Bus<T, W, R>, EffectWright) {
        let mut center_ticker = CenterTicker::new(self.rx);
        let rx1 = center_ticker.new_receiver();
        let rx2 = center_ticker.new_receiver();
        (
            Bus {
                center_ticker,
                shooter_ticker: ShooterTicker { rx: rx2 },
                subscribe_ticker: SubscribeTicker {
                    subs: self.subs,
                    rx: rx1,
                    err_queue: Arc::new(SegQueue::new()),
                },
                effect_ticker: EffectTicker {
                    tx: self.tx,
                    state_rx: self.state_rx,
                },
            },
            EffectWright {
                state_tx: self.state_tx,
            },
        )
    }
}

/// A wrapper for futures that can be safely sent between threads.
///
/// This struct provides a way to send futures between threads by implementing
/// the `Send` trait for any future type. It is marked as unsafe because the
/// implementation assumes the inner future can be safely sent between threads.
///
/// # Safety
/// The caller must ensure that the inner future can be safely sent between threads.
/// This is typically used when the future is known to be safe to send, but the
/// compiler cannot verify this automatically.
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// let future = async { /* ... */ };
/// let unsafe_future = UnsafeSendFuture(future);
/// tokio::spawn(unsafe_future);
/// ```
struct UnsafeSendFuture<F: Future>(F);

impl<F: Future> Future for UnsafeSendFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner_future = unsafe { self.map_unchecked_mut(|s| &mut s.0) };
        inner_future.poll(cx)
    }
}

unsafe impl<F: Future> Send for UnsafeSendFuture<F> {}
