//! Error handling module for the I/O event system.
//!
//! This module defines comprehensive error types for handling various failure cases in the event system:
//! - Event type conversion errors
//! - Event subscription and procedure call errors
//! - Event sending and receiving errors
//! - Bus communication errors
//!
//! # Error Hierarchy
//! The error system is organized into several main categories:
//! - [`TryFromEventError`]: Errors during event type conversion
//! - [`CallSubscribeError`]: Errors during event subscription and procedure calls
//! - [`BusSendError`]: Errors during event sending
//! - [`BusRecvError`]: Errors during event receiving
//! - [`BusError`]: General bus communication errors
//!
//! # Examples
//! ```rust
//! use ioevent::prelude::*;
//!
//! // Handling event conversion errors
//! match event.try_into() {
//!     Ok(event) => process_event(event),
//!     Err(TryFromEventError::Deserialize(e)) => handle_deserialize_error(e),
//!     Err(TryFromEventError::TagError(expected, actual)) => handle_tag_mismatch(expected, actual),
//!     Err(e) => handle_other_error(e),
//! }
//! ```
//!
//! For more examples and detailed error handling patterns, see the individual error type documentation.

use channels::serdes::Cbor;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::{event::EventData, util::CenterError};

/// Errors that can occur during event type conversion.
///
/// This enum represents various failure cases when converting between event types:
/// - Deserialization errors from CBOR data
/// - Tag mismatches between expected and actual event types
/// - Invalid event formats
/// - Infallible conversion errors
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// match event.try_into() {
///     Ok(event) => process_event(event),
///     Err(TryFromEventError::Deserialize(e)) => {
///         eprintln!("Failed to deserialize event: {}", e);
///     }
///     Err(TryFromEventError::TagError(expected, actual)) => {
///         eprintln!("Event tag mismatch: expected {}, got {}", expected, actual);
///     }
///     Err(e) => handle_other_error(e),
/// }
/// ```
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
/// This enum represents various errors that can occur when:
/// - Subscribing to events
/// - Making procedure calls
/// - Converting between event types
/// - Sending events through channels
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// match subscribe_to_event() {
///     Ok(()) => println!("Successfully subscribed"),
///     Err(CallSubscribeError::TryFrom(e)) => {
///         eprintln!("Failed to convert event: {}", e);
///     }
///     Err(CallSubscribeError::SendEvent(e)) => {
///         eprintln!("Failed to send event: {}", e);
///     }
///     Err(e) => handle_other_error(e),
/// }
/// ```
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
/// This enum represents errors that can occur during event sending:
/// - Channel send errors
/// - Event serialization errors
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// match bus.send_event(&event) {
///     Ok(()) => println!("Event sent successfully"),
///     Err(BusSendError::Send(e)) => {
///         eprintln!("Failed to send event through channel: {}", e);
///     }
///     Err(BusSendError::Serialize(e)) => {
///         eprintln!("Failed to serialize event: {}", e);
///     }
/// }
/// ```
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
/// This enum represents errors that can occur during event reception:
/// - Channel receive errors
/// - Broadcast receive errors
/// - Broadcast send errors
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// match bus.receive_event() {
///     Ok(event) => process_event(event),
///     Err(BusRecvError::Recv(e)) => {
///         eprintln!("Failed to receive event: {}", e);
///     }
///     Err(BusRecvError::BoardcastRecv(e)) => {
///         eprintln!("Failed to receive broadcast: {}", e);
///     }
///     Err(e) => handle_other_error(e),
/// }
/// ```
#[derive(Debug, Error)]
pub enum BusRecvError<R> {
    /// An error occurred while receiving data through the channel
    Recv(channels::error::RecvError<CborDeError, R>),
    /// An error occurred while receiving a broadcast
    BoardcastRecv(tokio::sync::broadcast::error::RecvError),
    /// An error occurred while sending a broadcast
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
impl<W, R> From<CenterError<R>> for BusError<W, R> {
    fn from(value: CenterError<R>) -> Self {
        match value {
            CenterError::Left(e) => BusError::SendError(e),
            CenterError::Right(e) => BusError::BusRecv(e),
        }
    }
}

pub use channels::error::*;
pub use ciborium::value::Error as CborValueError;
