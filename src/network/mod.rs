pub(crate) mod api;
pub(crate) mod wifi;

use alloc::boxed::Box;
use embassy_executor::Spawner;
use embassy_net::{Config as NetConfig, Stack, StackResources};
use esp_hal::clock::Clocks;
use esp_hal::peripherals::{RNG, TIMG1, WIFI};
use esp_hal::system::RadioClockControl;
use esp_hal::timer::TimerGroup;
use esp_hal::Rng;
use esp_wifi::wifi::{WifiDevice, WifiStaDevice};
use esp_wifi::{initialize, EspWifiInitFor};

use crate::config::Config;
use crate::error::{map_embassy_spawn_err, map_wifi_err, map_wifi_init_err, Result};
use crate::network::api::WEB_TASK_POOL_SIZE;

pub(crate) const STACK_POOL_SIZE: usize = WEB_TASK_POOL_SIZE + 3;

pub(crate) fn init(
    cfg: Config,
    wifi: WIFI,
    rng: RNG,
    timer_group: TimerGroup<TIMG1>,
    radio_clocks: RadioClockControl,
    clocks: &Clocks,
    spawner: &Spawner,
) -> Result<()> {
    let init = initialize(
        EspWifiInitFor::Wifi,
        timer_group.timer0,
        Rng::new(rng),
        radio_clocks,
        &clocks,
    )
    .map_err(map_wifi_init_err)?;

    let (wifi_interface, controller) =
        esp_wifi::wifi::new_with_mode(&init, wifi, WifiStaDevice).map_err(map_wifi_err)?;

    let config = NetConfig::dhcpv4(Default::default());
    let stack_resources = Box::leak(Box::new(StackResources::<STACK_POOL_SIZE>::new()));
    let seed = 1234; // very random, very secure seed

    let stack = Stack::new(wifi_interface, config, stack_resources, seed);
    let stack = Box::leak(Box::new(stack));

    spawner
        .spawn(net_stack(stack))
        .map_err(map_embassy_spawn_err)?;

    spawner
        .spawn(wifi::connection(cfg.clone(), stack, controller))
        .map_err(map_embassy_spawn_err)?;

    api::init(cfg, stack, spawner)?;

    Ok(())
}

#[embassy_executor::task]
pub async fn net_stack(stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>) {
    log::info!("Started: Network stack task");

    stack.run().await
}
