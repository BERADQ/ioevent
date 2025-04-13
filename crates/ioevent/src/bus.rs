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

pub mod state {
    use super::EffectWright;

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
}
