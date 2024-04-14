use picoserve::extract::{FromRequest, State};
use picoserve::io::Read;
use picoserve::request::{RequestBody, RequestParts};
use picoserve::response::Json;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::mister::{ChangeMode, Mode as MisterMode, ACTIVE_MODE};
use crate::network::api::types::OkResponse;
use crate::network::api::utils::deser_from_request;
use crate::network::api::ApiState;

pub(crate) async fn handle_get() -> Json<GetModeResponse> {
    Json(GetModeResponse {
        mode: ACTIVE_MODE.read().clone(),
    })
}

pub(crate) async fn handle_change(
    State(state): State<ApiState>,
    req: ChangeModeRequest,
) -> Result<Json<OkResponse>> {
    state
        .change_mode_pub
        .publish_immediate(ChangeMode::new(Some(req.mode)));

    Ok(Json(OkResponse::default()))
}

#[derive(Serialize)]
pub(crate) struct GetModeResponse {
    mode: Option<MisterMode>,
}

#[derive(Deserialize)]
pub(crate) struct ChangeModeRequest {
    mode: MisterMode,
}

impl<'r, State> FromRequest<'r, State> for ChangeModeRequest {
    type Rejection = Error;

    async fn from_request<R: Read>(
        _state: &'r State,
        _request_parts: RequestParts<'r>,
        request_body: RequestBody<'r, R>,
    ) -> Result<Self> {
        deser_from_request(request_body).await
    }
}
