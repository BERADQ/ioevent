use ioevent::{bus::state::*, error::TryFromEventError};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct CallPrint(pub String);

impl ProcedureCall for CallPrint {
    const PATH: &'static str = "com::demo::my::CallPrint";
}
impl ProcedureCallRequest for CallPrint {
    type RESPONSE = CallPrintResponse;
}
impl TryFrom<ProcedureCallData> for CallPrint {
    type Error = TryFromEventError;
    fn try_from(value: ProcedureCallData) -> Result<Self, Self::Error> {
        Ok(value.data.deserialized()?)
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct CallPrintResponse(pub i32);

impl ProcedureCall for CallPrintResponse {
    const PATH: &'static str = "com::demo::my::CallPrintResponse";
}
impl ProcedureCallResponse for CallPrintResponse {
    type REQUEST = CallPrint;
}
impl TryFrom<ProcedureCallData> for CallPrintResponse {
    type Error = TryFromEventError;
    fn try_from(value: ProcedureCallData) -> Result<Self, Self::Error> {
        Ok(value.data.deserialized()?)
    }
}

