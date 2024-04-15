use alloc::format;
use embedded_svc::io::asynch::Read;
use picoserve::extract::{FromRequest, State};
use picoserve::request::{RequestBody, RequestParts};
use picoserve::response::Json;

use crate::config::MutableConfigInstance;
use crate::error::Error;
use crate::network::api::types::OkResponse;
use crate::network::api::utils::deser_from_request;
use crate::network::api::ApiState;

pub(crate) async fn handle_get(State(state): State<ApiState>) -> Json<MutableConfigInstance> {
    Json(MutableConfigInstance::from(state.cfg.load().as_ref()))
}

pub(crate) async fn handle_update(
    State(state): State<ApiState>,
    req: MutableConfigInstance,
) -> crate::error::Result<Json<OkResponse>> {
    state.cfg.apply(req)?;

    Ok(Json(OkResponse::new(format!(
        "device will reset in {} seconds",
        state.cfg.load().reset_wait_secs
    ))))
}

pub(crate) async fn handle_reset(
    State(state): State<ApiState>,
) -> crate::error::Result<Json<OkResponse>> {
    state.cfg.reset()?;

    Ok(Json(OkResponse::new(format!(
        "device will reset in {} seconds",
        state.cfg.load().reset_wait_secs
    ))))
}

impl<'r, State> FromRequest<'r, State> for MutableConfigInstance {
    type Rejection = Error;

    async fn from_request<R: Read>(
        _state: &'r State,
        _request_parts: RequestParts<'r>,
        request_body: RequestBody<'r, R>,
    ) -> crate::error::Result<Self> {
        deser_from_request(request_body).await
    }
}
