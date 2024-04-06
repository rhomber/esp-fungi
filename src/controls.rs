use alloc::format;
use alloc::sync::Arc;
use core::fmt::{Display, Formatter};

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Timer};
use embedded_hal_async::digital::Wait;
use embedded_storage::{ReadStorage, Storage};
use esp_hal::gpio::{GpioPin, Input, PullDown, Unknown};
use esp_hal::prelude::_embedded_hal_digital_v2_InputPin;
use esp_storage::FlashStorage;
use esp_wifi::wifi::log_timestamp;
use spin::RwLock;

use crate::config::{Config, ConfigInstance};
use crate::error::{general_fault, map_embassy_spawn_err, map_infallible_err, Result};

const MODE_BUTTON_GPIO_PIN: u8 = 21;
const FLASH_STORAGE_ADDR: u32 = 0x9000;

pub(crate) static ACTIVE_MODE: RwLock<Option<Mode>> = RwLock::new(None);

#[derive(Copy, Clone)]
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

enum ButtonState {
    Pressed,
    Held,
    Released,
}

pub(crate) fn init(
    cfg: Config,
    mode_btn: GpioPin<Unknown, MODE_BUTTON_GPIO_PIN>,
    spawner: &Spawner,
) -> Result<()> {
    spawner
        .spawn(controls_task(cfg, mode_btn))
        .map_err(map_embassy_spawn_err)?;

    Ok(())
}

#[embassy_executor::task]
async fn controls_task(cfg: Config, mode_btn: GpioPin<Unknown, MODE_BUTTON_GPIO_PIN>) {
    let mut storage = FlashStorage::new();
    load_mode(&mut storage).await;

    let mut mode_btn = mode_btn.into_pull_down_input();

    loop {
        if let Err(e) = controls_task_poll(cfg.load(), &mut mode_btn, &mut storage).await {
            log::warn!("Failed to handle controls task poll: {:?}", e);
        }
    }
}

async fn controls_task_poll(
    cfg: Arc<ConfigInstance>,
    mode_btn: &mut GpioPin<Input<PullDown>, MODE_BUTTON_GPIO_PIN>,
    storage: &mut FlashStorage,
) -> Result<()> {
    mode_btn.wait_for_high().await.map_err(map_infallible_err)?;

    let start_ms = get_time_ms();

    loop {
        // Detect initial press threshold
        let _ = select(
            wait_for_low_of_ms(mode_btn, cfg.controls_min_press_ms),
            Timer::after(Duration::from_millis(cfg.controls_min_hold_ms as u64)),
        )
        .await;

        // Determine result (or if long press active)
        if mode_btn.is_high().map_err(map_infallible_err)? {
            if get_time_ms() - start_ms >= cfg.controls_min_hold_ms {
                handle_mode_button_event(ButtonState::Held, storage).await?;
                wait_for_low_of_ms(mode_btn, cfg.controls_min_press_ms).await?;
                handle_mode_button_event(ButtonState::Released, storage).await?;

                break;
            } else {
                continue;
            }
        } else {
            handle_mode_button_event(ButtonState::Pressed, storage).await?;
            break;
        }
    }

    Ok(())
}

async fn wait_for_low_of_ms(
    mode_btn: &mut GpioPin<Input<PullDown>, MODE_BUTTON_GPIO_PIN>,
    duration_ms: u32,
) -> Result<()> {
    loop {
        mode_btn.wait_for_low().await.map_err(map_infallible_err)?;

        match select(
            mode_btn.wait_for_high(),
            Timer::after(Duration::from_millis(duration_ms as u64)),
        )
        .await
        {
            Either::First(_) => {
                // Clear and start over.
                continue;
            }
            Either::Second(_) => {
                // Ok, we're done here.
                break;
            }
        }
    }

    Ok(())
}

async fn handle_mode_button_event(state: ButtonState, storage: &mut FlashStorage) -> Result<()> {
    match state {
        ButtonState::Pressed => {
            toggle_mode(storage).await?;
        }
        ButtonState::Held => {}
        ButtonState::Released => {}
    }

    Ok(())
}

async fn toggle_mode(storage: &mut FlashStorage) -> Result<()> {
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

    store_mode(storage, next_mode).await?;

    Ok(())
}

async fn load_mode(storage: &mut FlashStorage) {
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

    // TODO: Fire mode event.
}

async fn store_mode(storage: &mut FlashStorage, mode: Mode) -> Result<()> {
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

    // TODO: Fire mode event.

    Ok(())
}

fn get_time_ms() -> u32 {
    unsafe { log_timestamp() }
}
