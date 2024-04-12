mod status;

use picoserve::routing::{get, NoPathParameters, PathRouter};
use picoserve::Router;

use crate::error::Result;

pub(crate) fn init() -> Result<Router<impl PathRouter<(), NoPathParameters>>> {
    Ok(Router::new()
        .route("/", get(status::handle_get))
        .route("/status", get(status::handle_get)))
}
