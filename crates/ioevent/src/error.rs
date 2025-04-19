//! Error handling module for the I/O event system.
//!
//! Defines error types for event handling, conversion, and procedure calls.

use channels::serdes::Cbor;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::event::EventData;

/// Errors that can occur during event type conversion.
///
/// This enum represents various failure cases when converting between event types:
/// * Deserialization errors
/// * Tag mismatches
/// * Invalid event formats
/// * Infallible conversion errors
#[derive(Debug, Error)]
pub enum TryFromEventError {
    /// Error during event data deserialization
    #[error("Deserialize error: {0}")]
    Deserialize(ciborium::value::Error),
    /// Event tag mismatch
    #[error("Tag error: {0} != {1}")]
    TagError(String, String),
    /// Infallible conversion error
    #[error("?")]
    Infallible,
    /// Invalid event format
    #[error("Invalid event: {0}")]
    InvalidEvent(String),
}

impl From<String> for TryFromEventError {
    /// Converts a string into an InvalidEvent error.
    ///
    /// # Arguments
    /// * `value` - The error message string
    ///
    /// # Returns
    /// A TryFromEventError::InvalidEvent variant containing the message
    fn from(value: String) -> Self {
        TryFromEventError::InvalidEvent(value)
    }
}

/// Errors that can occur during event subscription and procedure calls.
///
/// This enum represents various errors that can occur when subscribing to events
/// or making procedure calls, including conversion errors, send errors, and
/// general procedure call errors.
#[derive(Debug, Error)]
pub enum CallSubscribeError {
    /// An error occurred during event conversion
    #[error("TryFrom error: {0}")]
    TryFrom(TryFromEventError),
    /// An error occurred while sending an event
    #[error("Send event error: {0}")]
    SendEvent(tokio::sync::mpsc::error::SendError<EventData>),
    /// A general error occurred
    #[error("CallSubscribe error: {0}")]
    Other(String),
    /// An error occurred during a procedure call
    #[error("Procedure call error: {0}")]
    ProcedureCall(String),
}

impl From<TryFromEventError> for CallSubscribeError {
    /// Converts a TryFromEventError into a CallSubscribeError.
    ///
    /// # Arguments
    /// * `value` - The TryFromEventError to convert
    ///
    /// # Returns
    /// A CallSubscribeError::TryFrom variant containing the original error
    fn from(value: TryFromEventError) -> Self {
        CallSubscribeError::TryFrom(value)
    }
}

impl From<ciborium::value::Error> for CallSubscribeError {
    /// Converts a ciborium value error into a CallSubscribeError.
    ///
    /// # Arguments
    /// * `value` - The ciborium value error to convert
    ///
    /// # Returns
    /// A CallSubscribeError::TryFrom variant containing a TryFromEventError::Deserialize
    fn from(value: ciborium::value::Error) -> Self {
        CallSubscribeError::TryFrom(TryFromEventError::Deserialize(value))
    }
}

impl From<tokio::sync::mpsc::error::SendError<EventData>> for CallSubscribeError {
    /// Converts a tokio mpsc send error into a CallSubscribeError.
    ///
    /// # Arguments
    /// * `value` - The send error to convert
    ///
    /// # Returns
    /// A CallSubscribeError::SendEvent variant containing the original error
    fn from(value: tokio::sync::mpsc::error::SendError<EventData>) -> Self {
        CallSubscribeError::SendEvent(value)
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for CallSubscribeError {
    /// Converts a tokio oneshot receive error into a CallSubscribeError.
    ///
    /// # Arguments
    /// * `value` - The receive error to convert
    ///
    /// # Returns
    /// A CallSubscribeError::ProcedureCall variant containing the error message
    fn from(value: tokio::sync::oneshot::error::RecvError) -> Self {
        CallSubscribeError::ProcedureCall(value.to_string())
    }
}

impl From<tokio::sync::oneshot::error::TryRecvError> for CallSubscribeError {
    /// Converts a tokio oneshot try receive error into a CallSubscribeError.
    ///
    /// # Arguments
    /// * `value` - The try receive error to convert
    ///
    /// # Returns
    /// A CallSubscribeError::ProcedureCall variant containing the error message
    fn from(value: tokio::sync::oneshot::error::TryRecvError) -> Self {
        CallSubscribeError::ProcedureCall(value.to_string())
    }
}

impl From<String> for CallSubscribeError {
    /// Converts a string into a CallSubscribeError.
    ///
    /// # Arguments
    /// * `value` - The error message string
    ///
    /// # Returns
    /// A CallSubscribeError::Other variant containing the message
    fn from(value: String) -> Self {
        CallSubscribeError::Other(value)
    }
}

type CborSeError = <Cbor as channels::serdes::Serializer<EventData>>::Error;

/// Errors that can occur when sending events through the bus.
///
/// This enum represents errors that can occur during event sending,
/// including serialization errors and channel send errors.
#[derive(Debug, Error)]
pub enum BusSendError<W> {
    /// An error occurred while sending data through the channel
    Send(channels::error::SendError<CborSeError, W>),
    /// An error occurred during event serialization
    Serialize(ciborium::value::Error),
}

impl<W> From<channels::error::SendError<CborSeError, W>> for BusSendError<W> {
    /// Converts a channel send error into a BusSendError.
    ///
    /// # Arguments
    /// * `value` - The send error to convert
    ///
    /// # Returns
    /// A BusSendError::Send variant containing the original error
    fn from(value: channels::error::SendError<CborSeError, W>) -> Self {
        BusSendError::Send(value)
    }
}

impl<W> From<ciborium::value::Error> for BusSendError<W> {
    /// Converts a ciborium value error into a BusSendError.
    ///
    /// # Arguments
    /// * `value` - The serialization error to convert
    ///
    /// # Returns
    /// A BusSendError::Serialize variant containing the original error
    fn from(value: ciborium::value::Error) -> Self {
        BusSendError::Serialize(value)
    }
}

type CborDeError = <Cbor as channels::serdes::Deserializer<EventData>>::Error;

/// Errors that can occur when receiving events through the bus.
///
/// This enum represents errors that can occur during event reception,
/// including channel receive errors.
#[derive(Debug, Error)]
pub enum BusRecvError<R> {
    /// An error occurred while receiving data through the channel
    Recv(channels::error::RecvError<CborDeError, R>),
    BoardcastRecv(tokio::sync::broadcast::error::RecvError),
    BoardcastSend(tokio::sync::broadcast::error::SendError<EventData>),
}

impl<R> From<tokio::sync::broadcast::error::RecvError> for BusRecvError<R> {
    fn from(value: tokio::sync::broadcast::error::RecvError) -> Self {
        BusRecvError::BoardcastRecv(value)
    }
}

impl<R> From<tokio::sync::broadcast::error::SendError<EventData>> for BusRecvError<R> {
    fn from(value: tokio::sync::broadcast::error::SendError<EventData>) -> Self {
        BusRecvError::BoardcastSend(value)
    }
}

impl<R> From<channels::error::RecvError<CborDeError, R>> for BusRecvError<R> {
    /// Converts a channel receive error into a BusRecvError.
    ///
    /// # Arguments
    /// * `value` - The receive error to convert
    ///
    /// # Returns
    /// A BusRecvError::Recv variant containing the original error
    fn from(value: channels::error::RecvError<CborDeError, R>) -> Self {
        BusRecvError::Recv(value)
    }
}

#[derive(Debug, Error)]
pub enum BusError<W, R> {
    BusSend(BusSendError<W>),
    CallSubscribe(CallSubscribeError),
    SendError(mpsc::error::SendError<EventData>),
    BusRecv(BusRecvError<R>),
}
impl<W, R> From<BusSendError<W>> for BusError<W, R> {
    fn from(value: BusSendError<W>) -> Self {
        BusError::BusSend(value)
    }
}
impl<W, R> From<CallSubscribeError> for BusError<W, R> {
    fn from(value: CallSubscribeError) -> Self {
        BusError::CallSubscribe(value)
    }
}
impl<W, R> From<mpsc::error::SendError<EventData>> for BusError<W, R> {
    fn from(value: mpsc::error::SendError<EventData>) -> Self {
        BusError::SendError(value)
    }
}
impl<W, R> From<BusRecvError<R>> for BusError<W, R> {
    fn from(value: BusRecvError<R>) -> Self {
        BusError::BusRecv(value)
    }
}

pub use channels::error::*;
pub use ciborium::value::Error as CborValueError;
