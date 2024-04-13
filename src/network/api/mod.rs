use alloc::boxed::Box;
use alloc::sync::Arc;

use embassy_executor::Spawner;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};
use esp_wifi::wifi::{WifiDevice, WifiStaDevice};
use picoserve::{KeepAlive, ShutdownMethod, Timeouts};

use crate::config::Config;
use crate::error::{map_embassy_pub_sub_err, map_embassy_spawn_err, Result};
use crate::mister::{ChangeModePublisher, CHANGE_MODE_CHANNEL};

mod routes;

// Only works with 1 at the moment (probs how the stack is shared).
pub(crate) const WEB_TASK_POOL_SIZE: usize = 1;

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
        connection: KeepAlive::Close,
        shutdown_method: ShutdownMethod::Shutdown,
    }));

    let change_mode_pub = Arc::new(
        CHANGE_MODE_CHANNEL
            .publisher()
            .map_err(map_embassy_pub_sub_err)?,
    );

    for id in 0..WEB_TASK_POOL_SIZE {
        spawner
            .spawn(web_task(
                id,
                cfg.clone(),
                stack,
                pico_cfg,
                change_mode_pub.clone(),
            ))
            .map_err(map_embassy_spawn_err)?;
    }

    Ok(())
}

#[embassy_executor::task(pool_size = WEB_TASK_POOL_SIZE)]
pub async fn web_task(
    id: usize,
    cfg: Config,
    stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>,
    pico_cfg: &'static picoserve::Config<Duration>,
    change_mode_pub: Arc<ChangeModePublisher>,
) {
    let app = routes::init(cfg, change_mode_pub).expect("Failed to init routes");

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
