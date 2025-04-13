use std::pin::Pin;

use crate::error::CallSubscribeError;

type FutureRet<T> = Pin<Box<(dyn Future<Output = T> + Send + Sync + 'static)>>;
pub type SubscribeFutureRet = FutureRet<Result<(), CallSubscribeError>>;
