use picoserve::routing::{get, post, PathRouter};
use picoserve::Router;

use crate::error::Result;
use crate::network::api::ApiState;

pub(crate) mod config;
pub(crate) mod mode;
pub(crate) mod status;

pub(crate) fn init() -> Result<Router<impl PathRouter<ApiState> + Sized, ApiState>> {
    Ok(Router::new()
        .route("/", get(status::handle_get))
        .route("/status", get(status::handle_get))
        .route("/mode", get(mode::handle_get))
        .route("/mode/change", post(mode::handle_change))
        .route("/config", get(config::handle_get))
        .route("/config/update", post(config::handle_update))
        // TODO>
        .route("/config/reset", post(config::handle_update)))
}
