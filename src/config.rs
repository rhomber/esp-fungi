use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU8, Ordering};

use crate::error::{general_fault, Result};

static MAX_SLOTS: u8 = 5;

static HOT_SLOT: AtomicU8 = AtomicU8::new(0);
static COLD_SLOT: AtomicU8 = AtomicU8::new(0);

static DEFAULT_SENSOR_DELAY_MS: u32 = 5000;
static DEFAULT_SENSOR_DELAY_ERR_MS: u32 = 10000;

#[derive(Clone)]
pub(crate) struct Config {
    slots: Arc<UnsafeCell<Vec<Arc<ConfigInstance>>>>,
}

impl Config {
    pub(crate) fn new(sensor_delay_ms: u32, sensor_delay_err_ms: u32) -> Result<Self> {
        let mut slots = Vec::new();
        slots.push(Arc::new(ConfigInstance::new(
            sensor_delay_ms,
            sensor_delay_err_ms,
        )));

        // Initialize.
        for _ in 1..MAX_SLOTS {
            slots.push(Arc::new(ConfigInstance::default()));
        }

        let s = Self {
            slots: Arc::new(UnsafeCell::new(slots)),
        };

        Ok(s)
    }

    pub(crate) fn load(&self) -> Result<Arc<ConfigInstance>> {
        let slot = HOT_SLOT.load(Ordering::Acquire);

        unsafe {
            self.slots
                .get()
                .as_ref()
                .ok_or(general_fault(format!(
                    "failed to get ref to slots (slot index: {})",
                    slot
                )))?
                .get(slot as usize)
                .ok_or(general_fault(format!(
                    "failed to get Config at slot (slot index: {})",
                    slot
                )))
                .map(|v| v.clone())
        }
    }

    #[allow(dead_code)]
    fn update(&self, new: Arc<ConfigInstance>) -> Result<()> {
        let slot = next_config_slot()?;

        unsafe {
            self.slots
                .get()
                .as_mut()
                .ok_or(general_fault(format!(
                    "failed to get mut to slots (slot index: {})",
                    slot
                )))?
                .insert(slot as usize, new);
        }

        HOT_SLOT.store(slot, Ordering::Release);

        Ok(())
    }
}

fn next_config_slot() -> Result<u8> {
    let mut attempts: u8 = 0;
    loop {
        let current_slot = COLD_SLOT.load(Ordering::Acquire);
        let next_slot = if current_slot + 1 > MAX_SLOTS {
            0
        } else {
            current_slot + 1
        };

        if COLD_SLOT
            .compare_exchange(
                current_slot,
                next_slot,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            return Ok(next_slot);
        } else {
            if attempts >= 100 {
                return Err(general_fault(format!(
                    "Max attempts ({}) exceeded acquiring slot trying to update Config",
                    attempts
                )));
            }

            attempts += 1;
        }
    }
}

pub(crate) struct ConfigInstance {
    pub(crate) wifi_ssid: String,
    pub(crate) wifi_password: String,
    pub(crate) sensor_delay_ms: u32,
    pub(crate) sensor_delay_err_ms: u32,
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
            sensor_delay_ms: DEFAULT_SENSOR_DELAY_MS,
            sensor_delay_err_ms: DEFAULT_SENSOR_DELAY_ERR_MS,
        }
    }
}
