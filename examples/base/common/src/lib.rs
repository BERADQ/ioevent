use serde::{Deserialize, Serialize};
use ioevent::prelude::*;


#[derive(Deserialize, Serialize, Debug, Event)]
pub struct A {
    pub foo: String,
    pub bar: i64,
}
#[derive(Deserialize, Serialize, Debug, Event)]
pub struct B {
    pub foo: i64,
    pub bar: String,
}
#[derive(Deserialize, Serialize, Debug, Event)]
// Custom tag
#[event(tag = "com::demo::my::C")]
pub struct C(pub String, pub i64);