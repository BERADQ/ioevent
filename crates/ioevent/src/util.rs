use std::marker::PhantomData;

use channels::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::{
    EventData,
    error::{BusError, BusRecvError, CallSubscribeError},
};

/// A macro for creating a subscriber for a given event type.
///
/// This macro generates a subscriber for a specific event type, allowing the system to
/// register and handle events of that type.
///
/// # Arguments
/// * `$func` - The function to create a subscriber for
///
/// # Returns
/// A subscriber for the given event type
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
pub type Result = core::result::Result<(), CallSubscribeError>;

pub enum CenterErrorIter<L, R>
where
    L: Iterator<Item = mpsc::error::SendError<EventData>>,
    R: AsyncRead + Unpin,
{
    Left(L),
    Right(Option<BusRecvError<R::Error>>),
}

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
