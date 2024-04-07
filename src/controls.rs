use alloc::sync::Arc;

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Timer};
use embedded_hal_async::digital::Wait;
use esp_hal::gpio::{GpioPin, Input, PullDown, Unknown};
use esp_hal::prelude::*;
use esp_wifi::wifi::log_timestamp;

use crate::config::{Config, ConfigInstance};
use crate::display::{ChangeMode as DisplayChangeMode, ChangeModePublisher, Mode};
use crate::error::{map_embassy_pub_sub_err, map_embassy_spawn_err, map_infallible_err, Result};
use crate::mister::{
    ChangeMode as MisterChangeMode, ChangeModePublisher as MisterChangeModePublisher,
};
use crate::{display, mister};

const MODE_BUTTON_GPIO_PIN: u8 = 21;

pub(crate) fn init(
    cfg: Config,
    mode_btn: GpioPin<Unknown, MODE_BUTTON_GPIO_PIN>,
    spawner: &Spawner,
) -> Result<()> {
    let display_change_mode_pub = display::CHANGE_MODE_CHANNEL
        .publisher()
        .map_err(map_embassy_pub_sub_err)?;
    let mister_change_mode_pub = mister::CHANGE_MODE_CHANNEL
        .publisher()
        .map_err(map_embassy_pub_sub_err)?;

    spawner
        .spawn(controls_task(
            cfg,
            mode_btn,
            display_change_mode_pub,
            mister_change_mode_pub,
        ))
        .map_err(map_embassy_spawn_err)?;

    Ok(())
}

#[embassy_executor::task]
async fn controls_task(
    cfg: Config,
    mode_btn: GpioPin<Unknown, MODE_BUTTON_GPIO_PIN>,
    mut display_change_mode_pub: ChangeModePublisher,
    mut mister_change_mode_pub: MisterChangeModePublisher,
) {
    let mut mode_btn = mode_btn.into_pull_down_input();

    loop {
        if let Err(e) = controls_task_poll(
            cfg.load(),
            &mut mode_btn,
            &mut display_change_mode_pub,
            &mut mister_change_mode_pub,
        )
        .await
        {
            log::warn!("Failed to handle controls task poll: {:?}", e);
        }
    }
}

async fn controls_task_poll(
    cfg: Arc<ConfigInstance>,
    mode_btn: &mut GpioPin<Input<PullDown>, MODE_BUTTON_GPIO_PIN>,
    display_change_mode_pub: &mut ChangeModePublisher,
    mister_change_mode_pub: &mut MisterChangeModePublisher,
) -> Result<()> {
    mode_btn.wait_for_high().await.map_err(map_infallible_err)?;

    log::info!("Mode button activated ...");

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
                handle_mode_button_event(
                    ButtonState::Held,
                    display_change_mode_pub,
                    mister_change_mode_pub,
                )
                .await?;
                wait_for_low_of_ms(mode_btn, cfg.controls_min_press_ms).await?;
                handle_mode_button_event(
                    ButtonState::Released,
                    display_change_mode_pub,
                    mister_change_mode_pub,
                )
                .await?;

                break;
            } else {
                continue;
            }
        } else {
            handle_mode_button_event(
                ButtonState::Pressed,
                display_change_mode_pub,
                mister_change_mode_pub,
            )
            .await?;
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

async fn handle_mode_button_event(
    state: ButtonState,
    display_change_mode_pub: &mut ChangeModePublisher,
    mister_change_mode_pub: &mut MisterChangeModePublisher,
) -> Result<()> {
    log::info!("Mode button event: {:?}", state);

    match state {
        ButtonState::Pressed => {
            mister_change_mode_pub.publish_immediate(MisterChangeMode::default());
        }
        ButtonState::Held => {
            display_change_mode_pub.publish_immediate(DisplayChangeMode::new(Some(Mode::Info)));
        }
        ButtonState::Released => {
            display_change_mode_pub.publish_immediate(DisplayChangeMode::new(None));
        }
    }

    Ok(())
}

// Models

#[derive(Copy, Clone, Debug)]
enum ButtonState {
    Pressed,
    Held,
    Released,
}

// Utils

fn get_time_ms() -> u32 {
    unsafe { log_timestamp() }
}
