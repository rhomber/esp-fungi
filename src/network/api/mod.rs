use alloc::boxed::Box;

use embassy_executor::Spawner;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};
use esp_wifi::wifi::{WifiDevice, WifiStaDevice};
use picoserve::{KeepAlive, ShutdownMethod, Timeouts};

use crate::config::Config;
use crate::error::{map_embassy_spawn_err, Result};

mod core;
mod routes;

pub(crate) const WEB_TASK_POOL_SIZE: usize = 8;

pub(crate) fn init(
    cfg: Config,
    stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>,
    spawner: &Spawner,
) -> Result<()> {
    let pico_cfg = Box::leak(Box::new(picoserve::Config {
        timeouts: Timeouts {
            start_read_request: Some(Duration::from_secs(5)),
            read_request: Some(Duration::from_secs(1)),
            write: Some(Duration::from_secs(1)),
        },
        connection: KeepAlive::KeepAlive,
        shutdown_method: ShutdownMethod::Shutdown,
    }));

    for id in 0..WEB_TASK_POOL_SIZE {
        spawner
            .spawn(web_task(id, cfg.clone(), stack, pico_cfg))
            .map_err(map_embassy_spawn_err)?;
    }

    Ok(())
}

#[embassy_executor::task(pool_size = WEB_TASK_POOL_SIZE)]
pub async fn web_task(
    id: usize,
    _cfg: Config,
    stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>,
    pico_cfg: &'static picoserve::Config<Duration>,
) {
    let app = routes::init().expect("Failed to init routes");

    let port = 80;
    let mut tcp_rx_buffer = [0; 1024];
    let mut tcp_tx_buffer = [0; 1024];
    let mut http_buffer = [0; 2048];

    log::info!("API worker[{}]: Started (waiting for WIFI...)", id);

    wait_for_net(stack).await;

    log::info!("API worker[{}]: Listening", id);

    picoserve::listen_and_serve(
        id,
        &app,
        pico_cfg,
        stack,
        port,
        &mut tcp_rx_buffer,
        &mut tcp_tx_buffer,
        &mut http_buffer,
    )
    .await
}

async fn wait_for_net(stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>) {
    loop {
        if stack.is_link_up() {
            break;
        }

        Timer::after(Duration::from_millis(500)).await;
    }

    loop {
        if stack.config_v4().is_some() {
            break;
        }

        Timer::after(Duration::from_millis(500)).await;
    }
}
