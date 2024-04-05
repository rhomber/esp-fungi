use alloc::format;
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
    slots: Arc<UnsafeCell<Vec<InnerConfig>>>,
}

impl Config {
    pub(crate) fn new(sensor_delay_ms: u32, sensor_delay_err_ms: u32) -> Result<Self> {
        let mut slots = Vec::new();
        slots.push(InnerConfig::new(sensor_delay_ms, sensor_delay_err_ms));

        // Initialize.
        for i in 1..MAX_SLOTS {
            slots.push(InnerConfig::default());
        }

        let s = Self {
            slots: Arc::new(UnsafeCell::new(slots)),
        };

        Ok(s)
    }

    fn active(&self) -> Result<&InnerConfig> {
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
        }
    }

    fn update(&self, new: InnerConfig) -> Result<()> {
        let mut slot: u8 = 0;
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
                slot = next_slot;
                break;
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

    // Accessors
    pub(crate) fn sensor_delay_ms(&self) -> u32 {
        match self.active() {
            Ok(a) => a.sensor_delay_ms,
            Err(e) => {
                log::warn!("Failed to get config ('sensor_delay_ms'): {:?}", e);

                DEFAULT_SENSOR_DELAY_MS
            }
        }
    }

    pub(crate) fn sensor_delay_err_ms(&self) -> u32 {
        match self.active() {
            Ok(a) => a.sensor_delay_err_ms,
            Err(e) => {
                log::warn!("Failed to get config ('sensor_delay_err_ms'): {:?}", e);

                DEFAULT_SENSOR_DELAY_ERR_MS
            }
        }
    }
}

struct InnerConfig {
    sensor_delay_ms: u32,
    sensor_delay_err_ms: u32,
}

impl InnerConfig {
    fn new(sensor_delay_ms: u32, sensor_delay_err_ms: u32) -> Self {
        Self {
            sensor_delay_ms,
            sensor_delay_err_ms,
        }
    }
}

impl Default for InnerConfig {
    fn default() -> Self {
        Self {
            sensor_delay_ms: DEFAULT_SENSOR_DELAY_MS,
            sensor_delay_err_ms: DEFAULT_SENSOR_DELAY_ERR_MS,
        }
    }
}
