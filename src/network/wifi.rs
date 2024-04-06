use alloc::format;
use alloc::string::ToString;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};
use esp_wifi::wifi::{
    AuthMethod, ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent,
    WifiStaDevice, WifiState,
};

use crate::error::{general_fault, Result};

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

#[embassy_executor::task]
pub async fn connection(
    stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>,
    mut controller: WifiController<'static>,
) {
    log::info!("Started: WIFI connection task");

    loop {
        if let Err(e) = connection_poll(stack, &mut controller).await {
            log::error!("Failed to poll WIFI connection status: {:?}", e);
            Timer::after(Duration::from_millis(10000)).await
        }
    }
}

async fn connection_poll(
    stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>,
    controller: &mut WifiController<'static>,
) -> Result<()> {
    match esp_wifi::wifi::get_wifi_state() {
        WifiState::StaConnected => {
            // wait until we're no longer connected
            controller.wait_for_event(WifiEvent::StaDisconnected).await;
            Timer::after(Duration::from_millis(5000)).await
        }
        _ => {}
    }

    let client_config = Configuration::Client(ClientConfiguration {
        ssid: SSID
            .try_into()
            .map_err(|e| general_fault(format!("failed to cast SSID: {:?}", e)))?,
        password: PASSWORD
            .try_into()
            .map_err(|e| general_fault(format!("failed to cast PASSWORD: {:?}", e)))?,
        ..Default::default()
    });

    controller
        .set_configuration(&client_config)
        .map_err(|e| general_fault(format!("failed to set configuration: {:?}", e)))?;
    log::info!(
        "WIFI device configured [SSID: {}, HW: {}]",
        SSID,
        stack.hardware_address()
    );

    if !matches!(controller.is_started(), Ok(true)) {
        controller
            .start()
            .await
            .map_err(|e| general_fault(format!("failed to start wifi: {:?}", e)))?;
        log::info!("WIFI device started");
    }

    log::info!("Connecting to WIFI SSID '{}'", SSID);

    controller.connect().await.map_err(|e| {
        general_fault(format!(
            "Failed to connect to WIFI SSID '{}': {:?}",
            SSID, e
        ))
    })?;

    // Wait to get an IP
    stack.wait_config_up().await;

    let ip_addr = stack
        .config_v4()
        .ok_or(general_fault("Failed to get config v4 from wifi stack".to_string()))?;

    log::info!(
        "Connected to WIFI: {:?}",
        ip_addr.address.address().to_string()
    );

    Ok(())
}
