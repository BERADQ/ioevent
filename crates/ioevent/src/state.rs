//! State module for managing application state and event shooters.
//!
//! This module provides components for state management in an event-driven system:
//! - Application state representation and access
//! - Event shooter management for selective event distribution
//! - Procedure call infrastructure for request-response patterns
//! - Utilities for waiting and responding to specific events
//!
//! # Architecture Overview
//! The state system consists of several key components:
//! - [`State`]: Manages application state and provides access to event shooters
//! - [`EventShooter`]: Selectively emits events based on specified criteria
//! - [`ProcedureCall`]: Infrastructure for request-response communication patterns
//! - [`ProcedureCallExt`]: Extensions for making and resolving procedure calls
//!
//! # Examples
//! ```rust
//! use ioevent::prelude::*;
//!
//! // Create a new state instance
//! let state = State::new(app_state, effect_wright);
//!
//! // Wait for a specific event
//! let event = state.wait_next::<MyEvent>().await?;
//!
//! // Make a procedure call
//! let response = state.call(&my_request).await?;
//! ```
//!
//! For more detailed examples and usage patterns, see the individual component documentation.

use std::{collections::HashMap, hash::Hash, ops::Deref, sync::Arc};

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
    pub fn create_with_selector(
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
    pub fn try_dispatch(self, event: &EventData) -> Option<Self> {
        if (self.selector)(event) {
            unsafe { self.force_dispatch(event) };
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
    pub unsafe fn force_dispatch(self, event: &EventData) -> bool {
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
    pub wright: EffectWright,
    /// Queue of event shooters waiting for specific events
    pub event_shooters: Arc<Mutex<Vec<EventShooter>>>,
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
            wright: bus,
            event_shooters: Arc::new(Mutex::new(Vec::new())),
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
        let event = self.wait_next_with(E::SELECTOR.0).await;
        let event = event.await?;
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
    pub async fn wait_next_with<F>(&self, selector: F) -> oneshot::Receiver<EventData>
    where
        F: Fn(&EventData) -> bool + Send + Sync + 'static,
    {
        let (shoot, rx) = EventShooter::create_with_selector(Box::new(selector));
        self.event_shooters.lock().await.push(shoot);
        rx
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
pub fn encode_procedure_call(path: &str, echo: u64, r#type: ProcedureCallType) -> String {
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
pub fn decode_procedure_call(path: &str) -> Result<(String, u64, ProcedureCallType), String> {
    let parts: Vec<&str> = path.split("\u{0000}|").collect();
    if parts.len() != 4 {
        return Err("Invalid procedure call path".to_string());
    }
    let path = parts[1].to_string();
    let echo = parts[3].parse().map_err(|_| "Invalid echo".to_string())?;
    let r#type = match parts[2] {
        "?echo=" => ProcedureCallType::Request,
        "!echo=" => ProcedureCallType::Response,
        _ => return Err("Invalid procedure call path".to_string()),
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
            payload: event::Value::serialized(&self)?,
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
            payload: event::Value::serialized(&self)?,
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
    pub payload: event::Value,
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
            tag: encode_procedure_call(&value.path, value.echo, value.r#type),
            payload: value.payload,
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
        crate::event::Selector(|e| e.tag.starts_with(Self::TAG));
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
        let (path, echo, r#type) = decode_procedure_call(&value.tag)?;
        Ok(ProcedureCallData {
            path,
            echo,
            r#type,
            payload: value.payload.clone(),
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
            .await;
        self.wright.emit(&request)?;
        let response = response.await?;
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
        self.wright.emit(&data)?;
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
    pub rng: Arc<Mutex<SmallRng>>,
}

impl Default for DefaultProcedureWright {
    /// Creates a new DefaultProcedureWright with a random seed.
    fn default() -> Self {
        Self {
            rng: Arc::new(Mutex::new(SmallRng::from_os_rng())),
        }
    }
}
/// A trait for generating unique echo identifiers for procedure calls.
pub trait ProcedureCallWright {
    /// Generates the next echo identifier.
    ///
    /// # Returns
    /// A future that resolves to a unique echo identifier
    fn next_echo(&self) -> impl Future<Output = u64> + Send + Sync;
}

impl ProcedureCallWright for DefaultProcedureWright {
    /// Generates the next echo identifier using the random number generator.
    ///
    /// This implementation:
    /// 1. Locks the random number generator
    /// 2. Generates a random u64
    /// 3. Returns it as the echo identifier
    async fn next_echo(&self) -> u64 {
        let mut rand = self.rng.lock().await;
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
        Ok(value.payload.deserialized()?)
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
        Ok(value.payload.deserialized()?)
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
        Ok(value.payload.deserialized()?)
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
        Ok(value.payload.deserialized()?)
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
                    Ok(value.payload.deserialized()?)
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
                Ok(value.payload.deserialized()?)
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
