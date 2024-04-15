use alloc::string::String;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct OkResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl OkResponse {
    pub(crate) fn new(message: String) -> Self {
        Self {
            message: Some(message),
            ..Self::default()
        }
    }
}

impl Default for OkResponse {
    fn default() -> Self {
        Self {
            success: true,
            message: None,
        }
    }
}
