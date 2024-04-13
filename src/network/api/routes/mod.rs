use alloc::sync::Arc;

use picoserve::routing::{get, NoPathParameters, PathRouter};
use picoserve::Router;

use crate::config::Config;
use crate::error::Result;
use crate::mister::ChangeModePublisher;

mod mode;
mod status;

pub(crate) fn init(
    _cfg: Config,
    _change_mode_pub: Arc<ChangeModePublisher>,
) -> Result<Router<impl PathRouter<(), NoPathParameters>>> {
    Ok(Router::new()
        .route("/", get(status::handle_get))
        .route("/status", get(status::handle_get))
        .route("/mode", get(mode::handle_get)))
    //.route(
    //    "/mode",
    //    post(move || {
    //        mode::handle_change(change_mode_pub.clone())
    //    }),
    //))
}
