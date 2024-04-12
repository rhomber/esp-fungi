use picoserve::response::Json;
use serde::Serialize;

use crate::mister::{Mode as MisterMode, Status as MisterStatus, ACTIVE_MODE, STATUS};
use crate::sensor::{SensorMetrics, METRICS};

pub(crate) async fn handle_get() -> Json<StatusResponse> {
    Json(StatusResponse {
        mode: ACTIVE_MODE.read().clone(),
        status: STATUS.read().clone(),
        metrics: METRICS.read().clone(),
    })
}

#[derive(Serialize)]
pub(crate) struct StatusResponse {
    mode: Option<MisterMode>,
    status: Option<MisterStatus>,
    metrics: Option<SensorMetrics>,
}
