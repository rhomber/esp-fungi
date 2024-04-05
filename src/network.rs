use embassy_net::{Config, Stack, StackResources};
use esp_hal::clock::Clocks;
use esp_hal::peripherals::{RNG, TIMG1, WIFI};
use esp_hal::system::RadioClockControl;
use esp_hal::timer::TimerGroup;
use esp_hal::Rng;
use esp_wifi::wifi::WifiStaDevice;
use esp_wifi::{initialize, EspWifiInitFor};
use static_cell::make_static;

use crate::error::{map_wifi_err, map_wifi_init_err, Result};

pub(crate) fn init(
    wifi: WIFI,
    rng: RNG,
    timer_group: TimerGroup<TIMG1>,
    radio_clocks: RadioClockControl,
    clocks: &Clocks,
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

    let config = Config::dhcpv4(Default::default());

    let seed = 1234; // very random, very secure seed

    // TODO: What is stack for? (the variable i mean)
    let _stack = &*make_static!(Stack::new(
        wifi_interface,
        config,
        make_static!(StackResources::<3>::new()),
        seed
    ));

    Ok(())
}
