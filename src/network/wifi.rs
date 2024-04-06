use embassy_time::{Duration, Timer};
use esp_wifi::wifi::{ClientConfiguration, Configuration, WifiController, WifiEvent, WifiState};
const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

#[embassy_executor::task]
pub async fn connection(mut controller: WifiController<'static>) {
    log::info!("Started: WIFI connection task");

    loop {
        match esp_wifi::wifi::get_wifi_state() {
            WifiState::StaConnected => {
                // wait until we're no longer connected
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                Timer::after(Duration::from_millis(5000)).await
            }
            _ => {}
        }

        if !matches!(controller.is_ap_enabled(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: SSID.try_into().unwrap(),
                password: PASSWORD.try_into().unwrap(),
                ..Default::default()
            });

            controller.set_configuration(&client_config).unwrap();
            log::info!("WIFI device configured");

            if !matches!(controller.is_started(), Ok(true)) {
                controller.start().await.unwrap();
                log::info!("WIFI device started");
            }
        }

        log::info!("Connecting to WIFI ...");

        match controller.connect().await {
            Ok(_) => log::info!("Connected to WIFI"),
            Err(e) => {
                log::warn!("Failed to connect to WIFI: {:?}", e);

                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}
