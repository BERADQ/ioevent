use crate::error::CallSubscribeError;

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
