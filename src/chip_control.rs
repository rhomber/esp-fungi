use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pubsub::{PubSubChannel, Publisher, Subscriber, WaitResult};
use embassy_time::{Duration, Timer};
use esp_hal::reset::software_reset;

use crate::config::{Config, ConfigInstance};
use crate::error::{map_embassy_pub_sub_err, map_embassy_spawn_err, Result};

pub(crate) type ChipControlPublisher =
    Publisher<'static, CriticalSectionRawMutex, ChipControlAction, 1, 1, 2>;
type ChipControlSubscriber =
    Subscriber<'static, CriticalSectionRawMutex, ChipControlAction, 1, 1, 2>;
pub(crate) static CHIP_CONTROL_CHANNEL: PubSubChannel<
    CriticalSectionRawMutex,
    ChipControlAction,
    1,
    1,
    2,
> = PubSubChannel::new();

pub(crate) fn init(cfg: Config, spawner: &Spawner) -> Result<()> {
    spawner
        .spawn(chip_control_task(
            cfg.clone(),
            CHIP_CONTROL_CHANNEL
                .subscriber()
                .map_err(map_embassy_pub_sub_err)?,
        ))
        .map_err(map_embassy_spawn_err)
}

#[embassy_executor::task]
async fn chip_control_task(cfg: Config, mut chip_control_sub: ChipControlSubscriber) {
    loop {
        if let Err(e) = chip_control_task_poll(cfg.load().as_ref(), &mut chip_control_sub).await {
            log::warn!("chip control task poll failed: {:?}", e);

            // Some sleep to avoid thrashing.
            Timer::after(Duration::from_millis(5000)).await;
        }
    }
}

async fn chip_control_task_poll(
    cfg: &ConfigInstance,
    chip_control_sub: &mut ChipControlSubscriber,
) -> Result<()> {
    match chip_control_sub.next_message().await {
        WaitResult::Lagged(count) => {
            log::warn!("chip control subscriber lagged by {} messages", count);

            // Ignore
            Ok(())
        }
        WaitResult::Message(action) => match action {
            ChipControlAction::Reset => {
                log::warn!("chip will reset in {} seconds ...", cfg.reset_wait_secs);
                Timer::after(Duration::from_secs(cfg.reset_wait_secs as u64)).await;
                software_reset();
                Ok(())
            }
        },
    }
}

#[derive(Clone)]
pub(crate) enum ChipControlAction {
    Reset,
}
