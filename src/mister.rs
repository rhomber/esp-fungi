use alloc::format;
use core::fmt::{Display, Formatter};

use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pubsub::{PubSubChannel, Publisher, Subscriber, WaitResult};
use embassy_time::{Duration, Timer};
use embedded_storage::{ReadStorage, Storage};
use esp_hal::gpio::{GpioPin, Output, PushPull, Unknown};
use esp_storage::FlashStorage;
use spin::RwLock;

use crate::config::Config;
use crate::error::{general_fault, map_embassy_pub_sub_err, map_embassy_spawn_err, Result};

const MISTER_POWER_GPIO_PIN: u8 = 17;
const STATUS_LED_GPIO_PIN: u8 = 22;
const FLASH_STORAGE_ADDR: u32 = 0x9000;

type ChangeModeSubscriber = Subscriber<'static, CriticalSectionRawMutex, ChangeMode, 1, 2, 1>;
pub(crate) type ChangeModePublisher =
    Publisher<'static, CriticalSectionRawMutex, ChangeMode, 1, 2, 1>;
pub(crate) static CHANGE_MODE_CHANNEL: PubSubChannel<CriticalSectionRawMutex, ChangeMode, 1, 2, 1> =
    PubSubChannel::new();

type ModeChangedPublisher = Publisher<'static, CriticalSectionRawMutex, Mode, 1, 1, 1>;
pub(crate) type ModeChangedSubscriber = Subscriber<'static, CriticalSectionRawMutex, Mode, 1, 1, 1>;
pub(crate) static MODE_CHANGED_CHANNEL: PubSubChannel<CriticalSectionRawMutex, Mode, 1, 1, 1> =
    PubSubChannel::new();

pub(crate) static ACTIVE_MODE: RwLock<Option<Mode>> = RwLock::new(None);

pub(crate) fn init(
    cfg: Config,
    mister_pwr_pin: GpioPin<Unknown, MISTER_POWER_GPIO_PIN>,
    status_led_pin: GpioPin<Unknown, STATUS_LED_GPIO_PIN>,
    spawner: &Spawner,
) -> crate::error::Result<()> {
    spawner
        .spawn(mister_operation_task(
            cfg.clone(),
            mister_pwr_pin,
            status_led_pin,
            MODE_CHANGED_CHANNEL
                .publisher()
                .map_err(map_embassy_pub_sub_err)?,
            CHANGE_MODE_CHANNEL
                .subscriber()
                .map_err(map_embassy_pub_sub_err)?,
        ))
        .map_err(map_embassy_spawn_err)?;

    Ok(())
}

#[embassy_executor::task]
async fn mister_operation_task(
    cfg: Config,
    mister_pwr_pin: GpioPin<Unknown, MISTER_POWER_GPIO_PIN>,
    status_led_pin: GpioPin<Unknown, STATUS_LED_GPIO_PIN>,
    mut mode_changed_pub: ModeChangedPublisher,
    mut change_mode_sub: ChangeModeSubscriber,
) {
    let mut storage = FlashStorage::new();
    load_mode(&mut storage, &mut mode_changed_pub).await;

    let mut mister_pwr_pin = mister_pwr_pin.into_push_pull_output();
    let mut status_led_pin = status_led_pin.into_push_pull_output();

    loop {
        if let Err(e) = mister_operation_task_poll(
            &cfg,
            &mut storage,
            &mut mister_pwr_pin,
            &mut status_led_pin,
            &mut mode_changed_pub,
            &mut change_mode_sub,
        )
        .await
        {
            log::warn!("mister operation task poll failed: {:?}", e);

            // Some sleep to avoid thrashing.
            Timer::after(Duration::from_millis(50)).await;
            continue;
        }
    }
}

async fn mister_operation_task_poll(
    _cfg: &Config,
    storage: &mut FlashStorage,
    mister_pwr_pin: &mut GpioPin<Output<PushPull>, MISTER_POWER_GPIO_PIN>,
    status_led_pin: &mut GpioPin<Output<PushPull>, STATUS_LED_GPIO_PIN>,
    mut mode_changed_pub: &mut ModeChangedPublisher,
    mut change_mode_sub: &mut ChangeModeSubscriber,
) -> Result<()> {
    match change_mode_sub.next_message().await {
        WaitResult::Lagged(count) => {
            return Err(general_fault(format!(
                "mister mode subscriber lagged by {} messages",
                count
            )));
        }
        WaitResult::Message(change_mode) => match change_mode.mode {
            Some(mode) => {
                store_mode(storage, mode, &mut mode_changed_pub).await?;

                // TODO: Apply mode.
            }
            None => {
                let mode = toggle_mode(storage, &mut mode_changed_pub).await?;

                // TODO: Apply mode.
            }
        },
    }

    Ok(())
}

async fn toggle_mode(
    storage: &mut FlashStorage,
    mode_changed_pub: &mut ModeChangedPublisher,
) -> Result<Mode> {
    let next_mode = match ACTIVE_MODE.read().clone() {
        None => Mode::Auto,
        Some(mode) => {
            let mode_u8 = mode as u8;
            if mode_u8 + 1 <= Mode::max() {
                Mode::from(mode_u8 + 1)
            } else {
                Mode::Auto
            }
        }
    };

    store_mode(storage, next_mode, mode_changed_pub).await?;

    Ok(next_mode)
}

async fn load_mode(storage: &mut FlashStorage, mode_changed_pub: &mut ModeChangedPublisher) {
    let mut bytes = [0u8; 1];
    let mode = match storage.read(FLASH_STORAGE_ADDR, &mut bytes) {
        Ok(_) => {
            let mode_u8 = u8::from_be_bytes(bytes);
            if mode_u8 >= Mode::min() && mode_u8 <= Mode::max() {
                let mode = Mode::from(u8::from_be_bytes(bytes));
                log::info!("Restored previous mode '{}' from flash", mode);
                mode
            } else {
                Mode::Auto
            }
        }
        Err(_) => Mode::Auto,
    };

    let _ = ACTIVE_MODE.write().insert(mode);
    mode_changed_pub.publish_immediate(mode);
}

async fn store_mode(
    storage: &mut FlashStorage,
    mode: Mode,
    mode_changed_pub: &mut ModeChangedPublisher,
) -> Result<()> {
    let mode_u8 = mode as u8;
    storage
        .write(FLASH_STORAGE_ADDR, mode_u8.to_be_bytes().as_ref())
        .map_err(|e| {
            general_fault(format!(
                "Failed to persist active mode to flash storage: {:?}",
                e
            ))
        })?;

    log::info!("Persisted mode '{}' to flash", mode);

    let _ = ACTIVE_MODE.write().insert(mode);
    mode_changed_pub.publish_immediate(mode);

    Ok(())
}

// Models

#[derive(Copy, Clone, PartialEq)]
pub(crate) enum Mode {
    Auto = 1,
    Off = 2,
    On = 3,
}

impl Mode {
    pub(crate) fn min() -> u8 {
        1
    }
    pub(crate) fn max() -> u8 {
        3
    }
}

impl Display for Mode {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            Mode::Auto => write!(f, "Auto"),
            Mode::Off => write!(f, "Off"),
            Mode::On => write!(f, "On"),
        }
    }
}

impl From<u8> for Mode {
    fn from(value: u8) -> Self {
        if value == 1 {
            Self::Auto
        } else if value == 2 {
            Self::Off
        } else if value == 3 {
            Self::On
        } else {
            Self::Auto
        }
    }
}

#[derive(Copy, Clone)]
pub(crate) struct ChangeMode {
    mode: Option<Mode>,
}

impl ChangeMode {
    pub(crate) fn new(mode: Option<Mode>) -> Self {
        Self { mode }
    }
}

impl Default for ChangeMode {
    fn default() -> Self {
        Self::new(None)
    }
}
