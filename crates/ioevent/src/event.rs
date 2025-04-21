//! Core event handling module for the I/O event system.
//!
//! This module provides the fundamental building blocks for event-driven communication:
//! - Event data structures and serialization
//! - Event type definitions and matching
//! - Subscriber management and event routing
//! - Event conversion and validation
//!
//! # Event System Overview
//! The event system is built around the following key concepts:
//! - Events are identified by a unique tag and contain CBOR-encoded data
//! - Subscribers are registered to handle specific event types
//! - Events can be converted between different representations
//! - Event matching is performed using selectors
//!
//! # Examples
//! ```rust
//! use ioevent::prelude::*;
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Event, Serialize, Deserialize)]
//! struct UserEvent {
//!     user_id: u64,
//!     action: String,
//! }
//! ```
//!
//! For more examples and detailed usage, see the individual type documentation.

use futures_util::future::join_all;
use std::{convert::Infallible, ops::Deref};

use serde::{Deserialize, Serialize};

use crate::{
    state::State,
    error::{CallSubscribeError, CborValueError, TryFromEventError},
    future::SubscribeFutureRet,
};

pub use ciborium::Value;

#[cfg(feature = "macros")]
pub use ioevent_macro::{Event, subscriber};

/// Raw event data structure containing event identifier and CBOR-encoded data.
///
/// This is the fundamental data structure used for event communication.
/// Events are identified by a string tag and contain arbitrary data encoded in CBOR format.
///
/// # Examples
/// ```rust
/// use ioevent::event::EventData;
/// use ciborium::Value;
///
/// let event = EventData {
///     event: "user.login".to_string(),
///     data: Value::Text("user123".to_string()),
/// };
/// ```
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct EventData {
    /// Unique identifier for the event type
    pub tag: String,
    /// Event payload encoded in CBOR format
    pub payload: Value,
}

/// Type alias for any event data
pub type AnyEvent = EventData;

/// Type alias for the subscription function signature
type SubscribeFn<T> = fn(&State<T>, &EventData) -> SubscribeFutureRet;

/// Event subscriber that processes events matching a specific selector.
///
/// A subscriber consists of:
/// - An event selector that determines which events to handle
/// - A handler function that processes matching events
/// - Type information for the event being handled
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// #[subscriber]
/// async fn handle_user_event(state: State<()>, event: UserEvent) -> Result {
///     println!("User event received: {:?}", event);
///     Ok(())
/// }
/// ```
pub struct Subscriber<T>((), &'static Selector, pub SubscribeFn<T>)
where
    T: 'static;

impl<T> Deref for Subscriber<T> {
    type Target = SubscribeFn<T>;
    fn deref(&self) -> &Self::Target {
        &self.2
    }
}

impl<T> Subscriber<T> {
    /// Creates a new subscriber for a specific event type.
    ///
    /// # Arguments
    /// * `f` - The subscription function to handle events
    pub const fn new<E>(f: SubscribeFn<T>) -> Self
    where
        E: Event,
    {
        Subscriber((), &E::SELECTOR, f)
    }

    /// Attempts to call the subscriber's handler function if the event matches.
    ///
    /// # Arguments
    /// * `state` - The current state
    /// * `event` - The event to process
    ///
    /// # Returns
    /// * `Ok(())` - If the event was processed successfully
    /// * `Err(CallSubscribeError)` - If an error occurred during processing
    pub async fn try_call(
        &self,
        state: &State<T>,
        event: &EventData,
    ) -> Result<(), CallSubscribeError> {
        if self.1.match_event(event) {
            (*self)(state, event).await
        } else {
            Ok(())
        }
    }
}

type InnerSubscribers<T> = Subscriber<T>;

/// A group of subscribers that can collectively handle events.
///
/// This structure manages multiple subscribers and provides functionality
/// to emit events to all registered subscribers.
pub struct Subscribers<T>(pub &'static [InnerSubscribers<T>])
where
    T: 'static;

impl<T> Subscribers<T>
where
    T: 'static,
{
    /// Initializes a new collection of subscribers.
    ///
    /// # Arguments
    /// * `sub_iter` - An iterator over subscribers to initialize
    pub fn init(sub_iter: impl Into<&'static [InnerSubscribers<T>]>) -> Self {
        Subscribers(sub_iter.into())
    }

    /// Emits an event to all subscribers.
    ///
    /// # Arguments
    /// * `state` - The current state
    /// * `event` - The event to emit
    ///
    /// # Returns
    /// An iterator over any errors that occurred during event processing
    pub async fn emit(
        &self,
        state: &State<T>,
        event: &EventData,
    ) -> impl Iterator<Item = CallSubscribeError> + use<T> + Send + 'static {
        let futures = self.0.iter().map(|sub| sub.try_call(state, event));
        join_all(futures).await.into_iter().filter_map(|v| v.err())
    }
}

impl From<Infallible> for TryFromEventError {
    fn from(_value: Infallible) -> Self {
        TryFromEventError::Infallible
    }
}

impl From<CborValueError> for TryFromEventError {
    fn from(value: CborValueError) -> Self {
        TryFromEventError::Deserialize(value)
    }
}

/// Trait defining the interface for event types in the system.
///
/// This trait must be implemented by all event types in the system. It provides:
/// - A unique tag for event identification
/// - Serialization and deserialization capabilities
/// - Type conversion between different event representations
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Event, Serialize, Deserialize)]
/// struct UserEvent {
///     user_id: u64,
///     action: String,
/// }
/// ```
pub trait Event: Serialize + for<'ed> TryFrom<&'ed EventData, Error = TryFromEventError> {
    /// Unique tag identifying this event type
    const TAG: &'static str;
    /// Selector used to match this event type
    const SELECTOR: Selector = Selector(|x| x.tag == Self::TAG);
    /// Converts the event into its raw EventData representation
    fn upcast(&self) -> Result<EventData, CborValueError> {
        Ok(EventData {
            tag: Self::TAG.to_string(),
            payload: Value::serialized(&self)?,
        })
    }
}

impl Event for EventData {
    const TAG: &'static str = "#";
    const SELECTOR: Selector = Selector(|_| true);
    fn upcast(&self) -> Result<EventData, CborValueError> {
        Ok(self.clone())
    }
}

impl TryFrom<&EventData> for EventData {
    type Error = TryFromEventError;
    fn try_from(value: &EventData) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

/// A selector used to match events based on specific criteria.
///
/// The selector contains a function that determines whether an event
/// matches certain conditions.
#[derive(Hash, Eq, PartialEq)]
pub struct Selector(pub fn(&EventData) -> bool);

impl Selector {
    /// Checks if the given event matches this selector's criteria
    fn match_event(&self, event: &EventData) -> bool {
        (self.0)(event)
    }
}
