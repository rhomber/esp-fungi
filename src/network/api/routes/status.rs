use core::ops::Deref;

use picoserve::extract::State;
use picoserve::response::{IntoResponse, Json};
use serde::Serialize;

use crate::config::ConfigInstance;
use crate::mister::{
    AutoScheduleMode, AutoScheduleState, Mode as MisterMode, Status as MisterStatus,
    ACTIVE_AUTO_SCHEDULE, ACTIVE_MODE, STATUS,
};
use crate::network::api::ApiState;
use crate::sensor::{SensorMetrics, METRICS};

pub(crate) async fn handle_get(State(state): State<ApiState>) -> impl IntoResponse {
    Json(StatusResponse {
        mode: ACTIVE_MODE.read().clone(),
        status: STATUS.read().clone(),
        active_auto_schedule: ActiveAutoSchedule::from(
            ACTIVE_AUTO_SCHEDULE.read().deref(),
            state.cfg.load().as_ref(),
        ),
        metrics: METRICS.read().clone(),
    })
}

#[derive(Serialize)]
pub(crate) struct StatusResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<MisterMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<MisterStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_auto_schedule: Option<ActiveAutoSchedule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metrics: Option<SensorMetrics>,
}

#[derive(Serialize)]
pub(crate) struct ActiveAutoSchedule {
    mode: AutoScheduleMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    idx: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rh: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_ms: Option<u32>,
}

impl ActiveAutoSchedule {
    fn from(state: &AutoScheduleState, cfg: &ConfigInstance) -> Option<Self> {
        match state.mode {
            AutoScheduleMode::Initial => Some(Self {
                mode: state.mode.clone(),
                idx: None,
                rh: None,
                remaining_ms: None,
                total_ms: None,
            }),
            AutoScheduleMode::Pending => {
                let sched = state.get_auto_schedule(cfg)?;

                Some(Self {
                    mode: state.mode.clone(),
                    idx: Some(state.idx),
                    rh: Some(sched.rh),
                    remaining_ms: None,
                    total_ms: Some(state.total_ms()),
                })
            }
            AutoScheduleMode::Running => {
                let sched = state.get_auto_schedule(cfg)?;

                Some(Self {
                    mode: state.mode.clone(),
                    idx: Some(state.idx),
                    rh: Some(sched.rh),
                    remaining_ms: Some(state.remaining_ms(cfg)?),
                    total_ms: Some(state.total_ms()),
                })
            }
        }
    }
}
