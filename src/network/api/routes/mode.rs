use picoserve::response::{IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::mister::{Mode as MisterMode, ACTIVE_MODE};

pub(crate) async fn handle_get() -> impl IntoResponse {
    Json(GetModeResponse {
        mode: ACTIVE_MODE.read().clone(),
    })
}

/*
pub(crate) async fn handle_change() -> impl IntoResponse {
    // TODO:

    Ok(handle_get().await)
}
 */

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
