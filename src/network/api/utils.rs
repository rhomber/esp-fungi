use crate::error::{general_fault, Result};
use alloc::format;
use embedded_svc::io::asynch::Read;
use picoserve::request::RequestBody;
use serde::de;

pub(crate) async fn deser_from_request<'r, T, R: Read>(
    request_body: RequestBody<'r, R>,
) -> Result<T>
where
    T: de::Deserialize<'r>,
{
    serde_json::from_slice(
        request_body
            .read_all()
            .await
            .map_err(|e| general_fault(format!("failed to read data from request: {:?}", e)))?,
    )
    .map_err(|e| general_fault(format!("failed to parse JSON from request: {:?}", e)))
}
