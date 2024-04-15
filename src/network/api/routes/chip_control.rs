use alloc::format;

use picoserve::extract::State;
use picoserve::response::Json;

use crate::chip_control::ChipControlAction;
use crate::network::api::types::OkResponse;
use crate::network::api::ApiState;

pub(crate) async fn handle_reset(
    State(state): State<ApiState>,
) -> crate::error::Result<Json<OkResponse>> {
    state
        .chip_control_pub
        .publish_immediate(ChipControlAction::Reset);

    Ok(Json(OkResponse::new(format!(
        "device will reset in {} seconds",
        state.cfg.load().reset_wait_secs
    ))))
}
