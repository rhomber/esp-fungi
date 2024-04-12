use picoserve::response::StatusCode;
use serde::Serialize;

use crate::mister::{Mode as MisterMode, Status as MisterStatus, ACTIVE_MODE, STATUS};
use crate::network::api::core::{json_response, BodyResponse};
use crate::sensor::{SensorMetrics, METRICS};

pub(crate) async fn handle_get() -> BodyResponse {
    json_response(
        StatusCode::OK,
        &StatusResponse {
            mode: ACTIVE_MODE.read().clone(),
            status: STATUS.read().clone(),
            metrics: METRICS.read().clone(),
        },
    )
}

#[derive(Serialize)]
struct StatusResponse {
    mode: Option<MisterMode>,
    status: Option<MisterStatus>,
    metrics: Option<SensorMetrics>,
}
