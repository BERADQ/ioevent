//! Future types module for the I/O event system.
//!
//! This module provides type aliases for futures used throughout the event system:
//! - [`FutureRet`]: A boxed future that can be sent and shared between threads
//! - [`SubscribeFutureRet`]: A future that returns a result from event subscription
//!
//! # Examples
//! ```rust
//! use ioevent::prelude::*;
//!
//! async fn handle_event(state: State<()>, event: EventData) -> SubscribeFutureRet {
//!     Box::pin(async move {
//!         // Process the event
//!         Ok(())
//!     })
//! }
//! ```
//!
//! For more examples and detailed usage, see the individual type documentation.

use std::future::Future;
use std::pin::Pin;

use crate::error::CallSubscribeError;

/// A boxed future that can be sent and shared between threads.
///
/// This type alias represents a pinned, boxed future that:
/// - Can be sent between threads (`Send`)
/// - Can be shared between threads (`Sync`)
/// - Returns a value of type `T`
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// async fn create_future() -> FutureRet<String> {
///     Box::pin(async move {
///         "Hello, world!".to_string()
///     })
/// }
/// ```
pub type FutureRet<T> = Pin<Box<(dyn Future<Output = T> + Send + Sync)>>;

/// A future that returns a result from event subscription.
///
/// This type alias represents a future that:
/// - Returns a `Result<(), CallSubscribeError>`
/// - Can be sent between threads
/// - Can be shared between threads
///
/// # Examples
/// ```rust
/// use ioevent::prelude::*;
///
/// async fn subscribe_to_event(_: State<()>, _: EventData) -> SubscribeFutureRet {
///     Box::pin(async move {
///         // Subscribe to the event
///         Ok(())
///     })
/// }
/// ```
pub type SubscribeFutureRet = FutureRet<Result<(), CallSubscribeError>>;
