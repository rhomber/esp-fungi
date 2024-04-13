use picoserve::response::Json;
use serde::Serialize;

use crate::mister::{Mode as MisterMode, Status as MisterStatus, ACTIVE_MODE, STATUS, ACTIVE_AUTO_RH};
use crate::sensor::{SensorMetrics, METRICS};

pub(crate) async fn handle_get() -> Json<StatusResponse> {
    Json(StatusResponse {
        mode: ACTIVE_MODE.read().clone(),
        status: STATUS.read().clone(),
        auto_rh: ACTIVE_AUTO_RH.read().clone(),
        metrics: METRICS.read().clone(),
    })
}

#[derive(Serialize)]
pub(crate) struct StatusResponse {
    mode: Option<MisterMode>,
    status: Option<MisterStatus>,
    auto_rh: Option<f32>,
    metrics: Option<SensorMetrics>,
}
