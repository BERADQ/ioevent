use channels::serdes::Cbor;
use thiserror::Error;

use crate::event::EventData;

#[derive(Debug, Error)]
pub enum TryFromEventError {
    #[error("Deserialize error: {0}")]
    Deserialize(ciborium::value::Error),
    #[error("Tag error: {0} != {1}")]
    TagError(String, String),
    #[error("?")]
    Infallible,
    #[error("Invalid event: {0}")]
    InvalidEvent(String),
}

#[derive(Debug, Error)]
pub enum CallSubscribeError {
    #[error("TryFrom error: {0}")]
    TryFrom(TryFromEventError),
    #[error("Send event error: {0}")]
    SendEvent(tokio::sync::mpsc::error::SendError<EventData>),
    #[error("CallSubscribe error: {0}")]
    Other(String),
    #[error("Procedure call error: {0}")]
    ProcedureCall(String),
}
impl From<TryFromEventError> for CallSubscribeError {
    fn from(value: TryFromEventError) -> Self {
        CallSubscribeError::TryFrom(value)
    }
}
impl From<ciborium::value::Error> for CallSubscribeError {
    fn from(value: ciborium::value::Error) -> Self {
        CallSubscribeError::TryFrom(TryFromEventError::Deserialize(value))
    }
}

impl From<tokio::sync::mpsc::error::SendError<EventData>> for CallSubscribeError {
    fn from(value: tokio::sync::mpsc::error::SendError<EventData>) -> Self {
        CallSubscribeError::SendEvent(value)
    }
}

impl From<String> for CallSubscribeError {
    fn from(value: String) -> Self {
        CallSubscribeError::Other(value)
    }
}

type CborSeError = <Cbor as channels::serdes::Serializer<EventData>>::Error;

#[derive(Debug, Error)]
pub enum BusSendError<W> {
    Send(channels::error::SendError<CborSeError, W>),
    Serialize(ciborium::value::Error),
}

impl<W> From<channels::error::SendError<CborSeError, W>> for BusSendError<W> {
    fn from(value: channels::error::SendError<CborSeError, W>) -> Self {
        BusSendError::Send(value)
    }
}

impl<W> From<ciborium::value::Error> for BusSendError<W> {
    fn from(value: ciborium::value::Error) -> Self {
        BusSendError::Serialize(value)
    }
}

type CborDeError = <Cbor as channels::serdes::Deserializer<EventData>>::Error;

#[derive(Debug, Error)]
pub enum BusRecvError<R> {
    Recv(channels::error::RecvError<CborDeError, R>),
}
impl<R> From<channels::error::RecvError<CborDeError, R>> for BusRecvError<R> {
    fn from(value: channels::error::RecvError<CborDeError, R>) -> Self {
        BusRecvError::Recv(value)
    }
}

pub use channels::error::*;
pub use ciborium::value::Error as CborValueError;
