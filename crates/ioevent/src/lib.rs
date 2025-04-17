#![doc = include_str!("../../../README.md")]
pub mod bus;
pub mod error;
pub mod event;
pub mod future;
pub mod util;

pub mod prelude {
    pub use crate::{
        bus::{Bus, BusBuilder, EffectTicker, EffectWright, IoPair, SubscribeTicker, state::State},
        create_subscriber,
        event::{EventData, Subscriber, Subscribers,Event},
    };
    #[cfg(feature = "macros")]
    pub use crate::event::subscriber;
}

pub use prelude::*;
