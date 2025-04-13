use futures_util::future::join_all;
use std::{convert::Infallible, ops::Deref};

use serde::{Deserialize, Serialize};

use crate::{
    bus::state::State,
    error::{CallSubscribeError, CborValueError, TryFromEventError},
    future::SubscribeFutureRet,
};

#[cfg(feature = "macros")]
pub use ioevent_macro::*;

/// Raw event.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct EventData {
    pub event: String,
    pub data: ciborium::Value,
}

pub type AnyEvent = EventData;

/// Subscribe function.
type SubscribeFn<T> = fn(&State<T>, &EventData) -> SubscribeFutureRet;

/// A subscriber.
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
    pub const fn new<E>(f: SubscribeFn<T>) -> Self
    where
        E: Event,
    {
        Subscriber((), &E::SELECTOR, f)
    }
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

/// A group of subscribers.
pub struct Subscribers<T>(pub &'static [InnerSubscribers<T>])
where
    T: 'static;

impl<T> Subscribers<T>
where
    T: 'static,
{
    /// Initialize subscribers.
    pub fn init(sub_iter: impl Into<&'static [InnerSubscribers<T>]>) -> Self {
        Subscribers(sub_iter.into())
    }
    /// Emit an event to all subscribers.
    pub async fn emit(
        &self,
        state: &State<T>,
        event: &EventData,
    ) -> impl Iterator<Item = CallSubscribeError> + use<T> {
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

pub trait Event: Serialize + for<'ed> TryFrom<&'ed EventData, Error = TryFromEventError> {
    const TAG: &'static str;
    const SELECTOR: Selector = Selector(|x| x.event == Self::TAG);
    fn upcast(&self) -> Result<EventData, CborValueError> {
        Ok(EventData {
            event: Self::TAG.to_string(),
            data: ciborium::Value::serialized(&self)?,
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

#[derive(Hash, Eq, PartialEq)]
pub struct Selector(pub fn(&EventData) -> bool);
impl Selector {
    fn match_event(&self, event: &EventData) -> bool {
        (self.0)(event)
    }
}
