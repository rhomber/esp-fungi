use alloc::string::{String, ToString};
use alloc::sync::Arc;

use spin::RwLock;

use crate::error::Result;

#[derive(Clone)]
pub(crate) struct Config {
    instance: Arc<RwLock<Option<Arc<ConfigInstance>>>>,
}

impl Config {
    #[allow(dead_code)]
    pub(crate) fn new(sensor_delay_ms: u32, sensor_delay_err_ms: u32) -> Self {
        Self::new_with_instance(ConfigInstance::new(sensor_delay_ms, sensor_delay_err_ms))
    }

    fn new_with_instance(inst: ConfigInstance) -> Self {
        Self {
            instance: Arc::new(RwLock::new(Some(Arc::new(inst)))),
        }
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

impl Default for Config {
    fn default() -> Self {
        Self::new_with_instance(ConfigInstance::default())
    }
}

pub(crate) struct ConfigInstance {
    pub(crate) wifi_ssid: String,
    pub(crate) wifi_password: String,
    pub(crate) display_enabled: bool,
    pub(crate) network_enabled: bool,
    pub(crate) sensor_enabled: bool,
    pub(crate) sensor_delay_ms: u32,
    pub(crate) sensor_delay_err_ms: u32,
    pub(crate) sensor_calibration_rh_adj: Option<f32>,
    pub(crate) controls_min_press_ms: u32,
    pub(crate) controls_min_hold_ms: u32,
    pub(crate) mister_auto_rh: f32,
    pub(crate) mister_auto_on_rh_adj: Option<f32>,
    pub(crate) mister_auto_duration_min_ms: u32,
}

impl ConfigInstance {
    #[allow(dead_code)]
    fn new(sensor_delay_ms: u32, sensor_delay_err_ms: u32) -> Self {
        Self {
            sensor_delay_ms,
            sensor_delay_err_ms,
            ..Self::default()
        }
    }

    pub(crate) fn mister_auto_on_rh(&self) -> f32 {
        match self.mister_auto_on_rh_adj {
            Some(adj) => self.mister_auto_rh + adj,
            None => self.mister_auto_rh,
        }
    }
}

impl Default for ConfigInstance {
    fn default() -> Self {
        Self {
            wifi_ssid: env!("SSID").to_string(),
            wifi_password: env!("PASSWORD").to_string(),
            display_enabled: true,
            network_enabled: true,
            sensor_enabled: true,
            sensor_delay_ms: 500,
            sensor_delay_err_ms: 10000,
            // Adjust for SHT45 which seems to be way higher than the others.
            sensor_calibration_rh_adj: Some(5.00),
            controls_min_press_ms: 100,
            controls_min_hold_ms: 500,
            mister_auto_rh: 88_f32,
            mister_auto_on_rh_adj: Some(-1_f32),
            mister_auto_duration_min_ms: 10000,
        }
    }
}
