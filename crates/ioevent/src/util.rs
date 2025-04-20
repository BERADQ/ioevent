//! Utility module providing helper types and macros for the I/O event system.
//!
//! This module contains various utility components that support the event system:
//! - Macros for creating subscribers
//! - Type aliases for common result types
//! - Error handling utilities
//! - Iterator implementations for error handling
//!
//! # Examples
//! ```rust
//! use ioevent::prelude::*;
//!
//! // Using the create_subscriber macro
//! #[subscriber]
//! async fn handle_event(state: State<()>, event: MyEvent) -> Result {
//!     println!("Event received: {:?}", event);
//!     Ok(())
//! }
//!
//! let subscriber = create_subscriber!(handle_event);
//! ```
//!
//! For more examples and detailed usage, see the individual component documentation.

//! - Macros for creating surs
//! - Type aliases for common result types

use channels::io::AsyncRead;
use tokio::sync::mpsc;

use crate::{
    EventData,
    error::{BusRecvError, CallSubscribeError},
};

/// A macro for creating a subscriber for a given event type.
///
/// This macro generates a subscriber for a specific event type, allowing the system to
/// register and handle events of that type. It simplifies the process of creating
/// subscribers by automatically handling the event type information. event: &MyEvent) -> Result<(), CallSubscribeError> {
///
/// # Arguments
/// * `$func` - The function to create a subscriber for
///
/// # Returns
/// A subscriber for the given event type
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// #[subscriber]
/// async fn handle_user_event(state: &State<()>, event: &UserEvent) -> Result<(), CallSubscribeError> {
///     println!("User event received: {:?}", event);
///     Ok(())
/// }
///
/// let subscriber = create_subscriber!(handle_user_event);
/// ```
/// A macro for creating a subscriber for a given event type.
#[macro_export]
macro_rules! create_subscriber {
    ($func:ident) => {
        ::ioevent::event::Subscriber::new::<$func::_Event>($func)
    };
}

/// A type alias for the result of a function that returns a CallSubscribeError.
///
/// This type alias simplifies the usage of the CallSubscribeError type, making it
/// easier to handle errors in a more concise manner.
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// async fn process_event(state: &State<()>, event: &EventData) -> Result {
///     // Process the event
///     Ok(())
/// }
/// ```
pub type Result = core::result::Result<(), CallSubscribeError>;

/// An iterator over center errors that can occur during event distribution.
///
/// This enum represents errors that can occur in the center ticker:
/// - Left variant: Errors from sending events through channels
/// - Right variant: Errors from receiving events from channels
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// for error in center_error_iter {
///     match error {
///         CenterError::Left(e) => handle_send_error(e),
///         CenterError::Right(e) => handle_recv_error(e),
///     }
/// }
/// ```
pub enum CenterErrorIter<L, R>
where
    L: Iterator<Item = mpsc::error::SendError<EventData>>,
    R: AsyncRead + Unpin,
{
    /// Iterator over send errors
    Left(L),
    /// Optional receive error
    Right(Option<BusRecvError<R::Error>>),
}

/// An error that can occur in the center ticker.
///
/// This enum represents the two types of errors that can occur:
/// - Left variant: Errors from sending events
/// - Right variant: Errors from receiving events
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// match center_error {
///     CenterError::Left(e) => handle_send_error(e),
///     CenterError::Right(e) => handle_recv_error(e),
/// }
/// ```
/// # Returns
/// An error occurred while sending an event
/// A subscriber for the given event type
/// An error occurred while receiving an event
///
pub enum CenterError<R> {
    Left(mpsc::error::SendError<EventData>),
    Right(BusRecvError<R>),
}

impl<I, R> Iterator for CenterErrorIter<I, R>
where
    R: AsyncRead + Unpin,
    I: Iterator<Item = mpsc::error::SendError<EventData>>,
{
    type Item = CenterError<R::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            CenterErrorIter::Left(iter) => iter.next().map(|e| CenterError::Left(e)),
            CenterErrorIter::Right(error) => error.take().map(|e| CenterError::Right(e)),
        }
    }
}
