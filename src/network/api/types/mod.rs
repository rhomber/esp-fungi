use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct OkResponse {
    success: bool,
}

impl Default for OkResponse {
    fn default() -> Self {
        Self { success: true }
    }
}
