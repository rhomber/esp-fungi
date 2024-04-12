use alloc::sync::Arc;

use picoserve::response::Json;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::mister::{ACTIVE_MODE, ChangeModePublisher, Mode as MisterMode};

pub(crate) async fn handle_get() -> Json<GetModeResponse> {
    Json(GetModeResponse {
        mode: ACTIVE_MODE.read().clone(),
    })
}

pub(crate) async fn handle_change(
    _change_mode_pub: Arc<ChangeModePublisher>,
) -> Result<Json<GetModeResponse>> {
    // TODO:

    Ok(handle_get().await)
}

#[derive(Serialize)]
pub(crate) struct GetModeResponse {
    mode: Option<MisterMode>,
}

#[derive(Deserialize)]
pub(crate) struct ChangeModeRequest {
    mode: MisterMode,
}

/*
impl<'r, State> FromRequest<'r, State> for ChangeModeResponse {
    type Rejection = Error;

    async fn from_request<R: Read>(
        _state: &'r State,
        _request_parts: RequestParts<'r>,
        request_body: RequestBody<'r, R>,
    ) -> Result<Self> {
        serde_json::from_slice(request_body.read_all().await.map_err(|e| {
            general_fault(format!(
                "failed to read data from change mode request: {:?}",
                e
            ))
        })?)
        .map_err(map_json_err)
    }
}
 */
