#![doc = include_str!("../README.md")]
pub mod bus;
pub mod error;
pub mod event;
pub mod future;
pub mod state;
pub mod util;

pub mod prelude {
    #[cfg(feature = "macros")]
    pub use crate::event::subscriber;
    pub use crate::{
        bus::{Bus, BusBuilder, EffectTicker, EffectWright, IoPair, SubscribeTicker},
        create_subscriber,
        event::{Event, EventData, Subscriber, Subscribers},
        state::State,
    };
}
pub mod rpc {
    pub use crate::state::*;
}

pub use prelude::*;
