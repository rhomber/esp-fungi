use alloc::string::{String, ToString};
use alloc::sync::Arc;
use spin::RwLock;

use crate::error::Result;

#[derive(Clone)]
pub(crate) struct Config {
    instance: Arc<RwLock<Option<Arc<ConfigInstance>>>>,
}

impl Config {
    pub(crate) fn new(sensor_delay_ms: u32, sensor_delay_err_ms: u32) -> Result<Self> {
        Ok(Self {
            instance: Arc::new(RwLock::new(Some(Arc::new(ConfigInstance::new(
                sensor_delay_ms,
                sensor_delay_err_ms,
            ))))),
        })
    }

    pub(crate) fn load(&self) -> Arc<ConfigInstance> {
        self.instance
            .read()
            .as_ref()
            .expect("failed to unwrap Config instance - should NEVER happen")
            .clone()
    }

    #[allow(dead_code)]
    pub(crate) fn update(&self, new: Arc<ConfigInstance>) -> Result<()> {
        let _ = self.instance.write().insert(new);

        Ok(())
    }
}

pub(crate) struct ConfigInstance {
    pub(crate) wifi_ssid: String,
    pub(crate) wifi_password: String,
    pub(crate) sensor_delay_ms: u32,
    pub(crate) sensor_delay_err_ms: u32,
    pub(crate) controls_min_press_ms: u32,
    pub(crate) controls_min_hold_ms: u32,
}

impl ConfigInstance {
    fn new(sensor_delay_ms: u32, sensor_delay_err_ms: u32) -> Self {
        Self {
            sensor_delay_ms,
            sensor_delay_err_ms,
            ..Self::default()
        }
    }
}

impl Default for ConfigInstance {
    fn default() -> Self {
        Self {
            wifi_ssid: env!("SSID").to_string(),
            wifi_password: env!("PASSWORD").to_string(),
            sensor_delay_ms: 5000,
            sensor_delay_err_ms: 10000,
            controls_min_press_ms: 150,
            controls_min_hold_ms: 1500,
        }
    }
}
