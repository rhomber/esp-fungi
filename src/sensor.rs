use alloc::format;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pubsub::{PubSubChannel, Publisher, Subscriber};
use embassy_time::{Duration, Timer};

use crate::config::Config;
#[cfg(feature = "hdc1080")]
use embedded_hdc1080_rs::Hdc1080;
use esp_hal::clock::Clocks;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::i2c::{Instance, I2C};
use esp_hal::peripheral::Peripheral;
use esp_hal::peripherals::I2C0;
use esp_hal::Delay;
use fugit::RateExtU32;

use crate::error::{general_fault, map_embassy_pub_sub_err, map_embassy_spawn_err, Result};

pub type SensorSubscriber =
    Subscriber<'static, CriticalSectionRawMutex, Option<ChannelMessage>, 1, 2, 1>;

pub(crate) static CHANNEL: PubSubChannel<CriticalSectionRawMutex, Option<ChannelMessage>, 1, 2, 1> =
    PubSubChannel::new();

pub(crate) fn init<SDA, SDA_, SCL, SCL_>(
    cfg: Config,
    sda: SDA,
    scl: SCL,
    i2c0: I2C0,
    clocks: &Clocks,
    spawner: &Spawner,
) -> Result<()>
where
    SDA: Peripheral<P = SDA_> + 'static,
    SDA_: InputPin + OutputPin,
    SCL: Peripheral<P = SCL_> + 'static,
    SCL_: InputPin + OutputPin,
{
    let delay = Delay::new(&clocks);

    let i2c = I2C::new(i2c0, sda, scl, 100.kHz(), &clocks);

    let dev = Device::new(i2c, delay)
        .map_err(|e| general_fault(format!("failed to create sensor device: {:?}", e)))?;

    spawner
        .spawn(emitter(
            cfg,
            dev,
            CHANNEL.publisher().map_err(map_embassy_pub_sub_err)?,
        ))
        .map_err(map_embassy_spawn_err)?;

    Ok(())
}

#[embassy_executor::task]
async fn emitter(
    cfg: Config,
    mut dev: Device<'static, I2C0>,
    publisher: Publisher<'static, CriticalSectionRawMutex, Option<ChannelMessage>, 1, 2, 1>,
) {
    loop {
        if let Err(e) = emitter_poll(&cfg, &mut dev, &publisher).await {
            log::warn!("Sensor emitter poll failed: {:?}", e);
        }
    }
}

async fn emitter_poll(
    cfg: &Config,
    dev: &mut Device<'static, I2C0>,
    publisher: &Publisher<'static, CriticalSectionRawMutex, Option<ChannelMessage>, 1, 2, 1>,
) -> Result<()> {
    let cfg = cfg.load()?;

    let msg = match dev.read() {
        Ok((temp, rh)) => {
            if temp > 0_f32 && rh > 0_f32 {
                log::debug!("Sensor - Temp: {}, RH: {}%", temp, rh);

                Some(ChannelMessage { temp, rh })
            } else {
                log::error!("Failed to read from sensor (temp: {}, rh: {})", temp, rh);

                None
            }
        }
        Err(e) => {
            log::error!("Failed to read from sensor: {:?}", e);

            None
        }
    };

    let is_ok = msg.is_some();

    publisher.publish_immediate(msg);

    if is_ok {
        Timer::after(Duration::from_millis(cfg.sensor_delay_ms as u64)).await;
    } else {
        Timer::after(Duration::from_millis(cfg.sensor_delay_err_ms as u64)).await;
    }

    Ok(())
}

#[derive(Clone)]
pub(crate) struct ChannelMessage {
    pub(crate) temp: f32,
    pub(crate) rh: f32,
}

#[cfg(feature = "hdc1080")]
struct Device<'d, T>
where
    T: Instance,
{
    dev: Hdc1080<I2C<'d, T>, Delay>,
}

#[cfg(feature = "hdc1080")]
impl<'d, T> Device<'d, T>
where
    T: Instance,
{
    fn new(i2c: I2C<'d, T>, delay: Delay) -> Result<Self> {
        let mut dev = Hdc1080::new(i2c, delay).map_err(|e| {
            general_fault(format!("failed to create hdc1080 sensor device: {:?}", e))
        })?;

        dev.init()
            .map_err(|e| general_fault(format!("failed to init hdc1080 sensor device: {:?}", e)))?;

        Ok(Self { dev })
    }

    fn read(&mut self) -> Result<(f32, f32)> {
        self.dev.read().map_err(|e| {
            general_fault(format!(
                "failed to read from hdc1080 sensor device: {:?}",
                e
            ))
        })
    }
}
