use ioevent::rpc::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, ProcedureCall)]
// Specifies a custom procedure path.
#[procedure(path = "com::demo::my::CallPrint")]
pub struct CallPrint(pub String);
impl ProcedureCallRequest for CallPrint {
    type RESPONSE = CallPrintResponse;
}

#[derive(Deserialize, Serialize, Debug, ProcedureCall)]
pub struct CallPrintResponse(pub u64);
impl ProcedureCallResponse for CallPrintResponse {}

#[derive(Deserialize, Serialize, Debug, ioevent::Event)]
pub struct EmptyEvent;