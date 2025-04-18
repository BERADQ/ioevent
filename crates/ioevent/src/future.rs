//! A module for types of futures.

use std::future::Future;
use std::pin::Pin;

use crate::error::CallSubscribeError;

pub type FutureRet<T> = Pin<Box<(dyn Future<Output = T> + Send + Sync)>>;
pub type SubscribeFutureRet = FutureRet<Result<(), CallSubscribeError>>;
