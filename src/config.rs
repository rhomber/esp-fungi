use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::{format, vec};

use embedded_storage::{ReadStorage, Storage};
use esp_storage::FlashStorage;
use serde::{Deserialize, Serialize};
use spin::RwLock;

use crate::chip_control;
use crate::chip_control::{ChipControlAction, ChipControlPublisher};
use crate::error::{general_fault, map_embassy_pub_sub_err, Result};

const CONFIG_LEN_FLASH_ADDR: u32 = 0x9200;
const CONFIG_DATA_FLASH_ADDR: u32 = 0x9202;
const MAX_CONFIG_DATA_LEN: usize = (16_usize.pow(2) * 8) - 2; // To 0x9900

type FlashStorageArc = Arc<RwLock<FlashStorage>>;

macro_rules! schedule {
    ($rh:expr, $run_secs:expr, $max_wait_secs:expr) => {
        MisterAutoSchedule::new($rh, $run_secs, $max_wait_secs)
    };
}

#[derive(Clone)]
pub(crate) struct Config {
    instance: Arc<RwLock<Option<Arc<ConfigInstance>>>>,
    chip_control_pub: Arc<ChipControlPublisher>,
    flash_storage: FlashStorageArc,
}

impl Config {
    pub(crate) fn new() -> Result<Self> {
        let mut flash_storage = Arc::new(RwLock::new(FlashStorage::new()));
        let inst = revive_from_flash(&mut flash_storage, ConfigInstance::default())?;

        Ok(Self {
            instance: Arc::new(RwLock::new(Some(Arc::new(inst)))),
            chip_control_pub: Arc::new(
                chip_control::CHIP_CONTROL_CHANNEL
                    .publisher()
                    .map_err(map_embassy_pub_sub_err)?,
            ),
            flash_storage,
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
        persist_to_flash(&self.flash_storage, &update)?;

        let mut new = ConfigInstance::default();
        if let Err(e) = update.populate(&mut new) {
            let _ = reset_config_flash(&self.flash_storage);
            return Err(e);
        }

        self.chip_control_pub
            .publish_immediate(ChipControlAction::Reset);

        self.update(Arc::new(new))
    }

    pub(crate) fn reset(&self) -> Result<()> {
        reset_config_flash(&self.flash_storage)?;

        self.chip_control_pub
            .publish_immediate(ChipControlAction::Reset);

        self.update(Arc::new(ConfigInstance::default()))
    }
}

fn revive_from_flash(
    flash_storage: &FlashStorageArc,
    mut inst: ConfigInstance,
) -> Result<ConfigInstance> {
    let mut bytes = [0u8; 2];
    let mut storage = flash_storage.write();

    // Read config length
    storage
        .read(CONFIG_LEN_FLASH_ADDR, &mut bytes)
        .map_err(|e| {
            general_fault(format!(
                "Failed to load config len field from flash storage: {:?}",
                e
            ))
        })?;

    let len = u16::from_be_bytes(bytes);
    if len == u16::MAX {
        // No persisted config.
        return Ok(inst);
    }

    let mut bytes = vec![0u8; len as usize];

    // Read config data
    storage
        .read(CONFIG_DATA_FLASH_ADDR, &mut bytes)
        .map_err(|e| {
            general_fault(format!(
                "Failed to load config data field from flash storage: {:?}",
                e
            ))
        })?;

    log::info!("Loaded config data from flash [{} bytes]", bytes.len());

    let data: MutableConfigInstance = ciborium::from_reader(bytes.as_slice()).map_err(|e| {
        general_fault(format!(
            "Failed to deserialize config data read from flash storage: {:?}",
            e
        ))
    })?;

    data.populate(&mut inst)?;
    Ok(inst)
}

fn persist_to_flash(
    flash_storage: &FlashStorageArc,
    mutable_cfg: &MutableConfigInstance,
) -> Result<()> {
    let mut bytes = Vec::new();
    ciborium::into_writer(mutable_cfg, &mut bytes).map_err(|e| {
        general_fault(format!(
            "Failed to serialize config data read for storage: {:?}",
            e
        ))
    })?;

    if bytes.len() > MAX_CONFIG_DATA_LEN {
        return Err(general_fault(format!(
            "Failed to serialize config data read for storage - max bytes exceeded: '{}' > '{}'",
            bytes.len(),
            MAX_CONFIG_DATA_LEN
        )));
    }

    write_config_len_to_flash(flash_storage, bytes.len() as u16)?;
    write_config_data_to_flash(flash_storage, &bytes)?;

    log::info!(
        "Wrote config data to flash [{} bytes of {} max]",
        bytes.len(),
        MAX_CONFIG_DATA_LEN
    );

    Ok(())
}

fn reset_config_flash(flash_storage: &FlashStorageArc) -> Result<()> {
    write_config_len_to_flash(flash_storage, u16::MAX)
}

fn write_config_len_to_flash(flash_storage: &FlashStorageArc, cfg_len: u16) -> Result<()> {
    let mut flash_storage = flash_storage.write();

    flash_storage
        .write(CONFIG_LEN_FLASH_ADDR, cfg_len.to_be_bytes().as_ref())
        .map_err(|e| {
            general_fault(format!(
                "Failed to write config len field to flash storage: {:?}",
                e
            ))
        })
}

fn write_config_data_to_flash(flash_storage: &FlashStorageArc, cfg_data: &[u8]) -> Result<()> {
    let mut flash_storage = flash_storage.write();

    flash_storage
        .write(CONFIG_DATA_FLASH_ADDR, cfg_data)
        .map_err(|e| {
            general_fault(format!(
                "Failed to write config data field to flash storage: {:?}",
                e
            ))
        })
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
    HDC1080,
}
