use std::pin::Pin;

use crate::error::CallSubscribeError;

pub type FutureRet<T> = Pin<Box<(dyn Future<Output = T>)>>;
pub type SubscribeFutureRet = FutureRet<Result<(), CallSubscribeError>>;
