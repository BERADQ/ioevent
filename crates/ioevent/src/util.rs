use crate::error::CallSubscribeError;

#[macro_export]
macro_rules! create_subscriber {
    ($func:ident) => {
        ::ioevent::event::Subscriber::new::<$func::_Event>($func)
    };
}

pub type Result = core::result::Result<(), CallSubscribeError>;
