use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use crate::chip_control;
use serde::{Deserialize, Serialize};
use spin::RwLock;

use crate::chip_control::{ChipControlAction, ChipControlPublisher};
use crate::error::{map_embassy_pub_sub_err, Result};

macro_rules! schedule {
    ($rh:expr, $run_secs:expr, $max_wait_secs:expr) => {
        MisterAutoSchedule::new($rh, $run_secs, $max_wait_secs)
    };
}

#[derive(Clone)]
pub(crate) struct Config {
    instance: Arc<RwLock<Option<Arc<ConfigInstance>>>>,
    chip_control_pub: Arc<ChipControlPublisher>,
}

impl Config {
    pub(crate) fn new() -> Result<Self> {
        // TODO: LOAD PERSIST

        Self::new_with_instance(ConfigInstance::default())
    }

    fn new_with_instance(inst: ConfigInstance) -> Result<Self> {
        Ok(Self {
            instance: Arc::new(RwLock::new(Some(Arc::new(inst)))),
            chip_control_pub: Arc::new(
                chip_control::CHIP_CONTROL_CHANNEL
                    .publisher()
                    .map_err(map_embassy_pub_sub_err)?,
            ),
        })
    }

    pub(crate) fn load(&self) -> Arc<ConfigInstance> {
        self.instance
            .read()
            .as_ref()
            .expect("failed to unwrap Config instance - should NEVER happen")
            .clone()
    }

    fn update(&self, new: Arc<ConfigInstance>) -> Result<()> {
        let _ = self.instance.write().insert(new);

        Ok(())
    }

    pub(crate) fn apply(&self, update: MutableConfigInstance) -> Result<()> {
        let mut new = ConfigInstance::default();
        update.populate(&mut new)?;

        // TODO: PERSIST.

        self.chip_control_pub.publish_immediate(ChipControlAction::Reset);

        self.update(Arc::new(new))
    }
}

#[derive(Clone)]
pub(crate) struct ConfigInstance {
    pub(crate) wifi_ssid: String,
    pub(crate) wifi_password: String,
    pub(crate) display_enabled: bool,
    pub(crate) network_enabled: bool,
    pub(crate) sensor_enabled: bool,
    pub(crate) sensor_driver: SensorDriver,
    pub(crate) sensor_delay_ms: u32,
    pub(crate) sensor_delay_err_ms: u32,
    pub(crate) sensor_calibration_rh_adj: Option<f32>,
    pub(crate) controls_min_press_ms: u32,
    pub(crate) controls_min_hold_ms: u32,
    pub(crate) mister_auto_schedule: Vec<MisterAutoSchedule>,
    pub(crate) mister_auto_on_rh_adj: Option<f32>,
    pub(crate) mister_auto_off_rh_adj: Option<f32>,
    pub(crate) mister_auto_duration_min_ms: u32,
    pub(crate) reset_wait_secs: u32,
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

    pub(crate) fn mister_auto_on_rh(&self, rh: f32) -> f32 {
        match self.mister_auto_on_rh_adj {
            Some(adj) => rh + adj,
            None => rh,
        }
    }

    pub(crate) fn mister_auto_off_rh(&self, rh: f32) -> f32 {
        match self.mister_auto_off_rh_adj {
            Some(adj) => rh + adj,
            None => rh,
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
            sensor_driver: SensorDriver::default(),
            sensor_delay_ms: 500,
            sensor_delay_err_ms: 10000,
            // Adjust for SHT45 which seems to be way higher than the others.
            sensor_calibration_rh_adj: Some(5.0),
            controls_min_press_ms: 100,
            controls_min_hold_ms: 500,
            mister_auto_schedule: vec![
                schedule![85.00, 60 * 2, Some(60 * 5)],
                schedule![88.00, 60 * 3, Some(60)],
                schedule![90.00, 60 * 4, Some(60)],
                schedule![92.00, 60 * 4, Some(60)],
                schedule![85.00, 60 * 2, Some(60 * 5)],
                schedule![80.00, 60 * 5, Some(60)],
            ],
            mister_auto_on_rh_adj: Some(-0.5),
            mister_auto_off_rh_adj: Some(0.5),
            mister_auto_duration_min_ms: 10000,
            reset_wait_secs: 5,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct MutableConfigInstance {
    pub(crate) sensor_driver: Option<SensorDriver>,
    pub(crate) sensor_calibration_rh_adj: Option<f32>,
    pub(crate) mister_auto_schedule: Option<Vec<MisterAutoSchedule>>,
    pub(crate) mister_auto_on_rh_adj: Option<f32>,
    pub(crate) mister_auto_off_rh_adj: Option<f32>,
}

impl MutableConfigInstance {
    #[allow(dead_code)]
    pub(crate) fn new() -> Self {
        Self {
            sensor_driver: None,
            sensor_calibration_rh_adj: None,
            mister_auto_schedule: None,
            mister_auto_on_rh_adj: None,
            mister_auto_off_rh_adj: None,
        }
    }

    pub(crate) fn populate(mut self, cfg: &mut ConfigInstance) -> Result<()> {
        if let Some(val) = self.sensor_driver.take() {
            cfg.sensor_driver = val;
        }
        if let Some(val) = self.sensor_calibration_rh_adj.take() {
            cfg.sensor_calibration_rh_adj = Some(val);
        }
        if let Some(val) = self.mister_auto_schedule.take() {
            cfg.mister_auto_schedule = val;
        }
        if let Some(val) = self.mister_auto_on_rh_adj.take() {
            cfg.mister_auto_on_rh_adj = Some(val);
        }
        if let Some(val) = self.mister_auto_off_rh_adj.take() {
            cfg.mister_auto_off_rh_adj = Some(val);
        }

        Ok(())
    }
}

impl From<&ConfigInstance> for MutableConfigInstance {
    fn from(value: &ConfigInstance) -> Self {
        Self {
            sensor_driver: Some(value.sensor_driver.clone()),
            sensor_calibration_rh_adj: value.sensor_calibration_rh_adj.clone(),
            mister_auto_schedule: Some(value.mister_auto_schedule.clone()),
            mister_auto_on_rh_adj: value.mister_auto_on_rh_adj.clone(),
            mister_auto_off_rh_adj: value.mister_auto_off_rh_adj.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct MisterAutoSchedule {
    pub(crate) rh: f32,
    pub(crate) run_secs: u32,
    pub(crate) max_wait_secs: Option<u32>,
}

impl MisterAutoSchedule {
    pub(crate) fn new(rh: f32, run_secs: u32, max_wait_secs: Option<u32>) -> Self {
        Self {
            rh,
            run_secs,
            max_wait_secs,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) enum SensorDriver {
    #[default]
    SHT40,
    Hdc1080,
}
