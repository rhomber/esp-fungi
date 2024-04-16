use alloc::format;
use core::cell::RefCell;

use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pubsub::{PubSubChannel, Publisher, Subscriber};
use embassy_time::{Duration, Timer};
use embedded_hal_bus::i2c::RefCellDevice;
#[cfg(feature = "hdc1080")]
use embedded_hdc1080_rs::Hdc1080;
use esp_hal::clock::Clocks;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::i2c::{Instance, I2C};
use esp_hal::peripheral::Peripheral;
use esp_hal::peripherals::I2C0;
use esp_hal::Delay;
use fugit::RateExtU32;
#[cfg(feature = "sht40")]
use sensor_temp_humidity_sht40::{I2CAddr, Precision, SHT40Driver, TempUnit};
use serde::Serialize;
use spin::RwLock;

use crate::config::{Config, ConfigInstance, SensorDriver};
use crate::error::{
    general_fault, map_embassy_pub_sub_err, map_embassy_spawn_err, sensor_fault, Result,
};

static MAX_RH: f32 = 100_f32;

pub(crate) static METRICS: RwLock<Option<SensorMetrics>> = RwLock::new(None);

pub type SensorSubscriber =
    Subscriber<'static, CriticalSectionRawMutex, Option<SensorMetrics>, 1, 2, 1>;

pub(crate) static CHANNEL: PubSubChannel<CriticalSectionRawMutex, Option<SensorMetrics>, 1, 2, 1> =
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
    spawner
        .spawn(emitter(
            cfg,
            I2C::new(i2c0, sda, scl, 1.kHz(), &clocks),
            Delay::new(clocks),
            CHANNEL.publisher().map_err(map_embassy_pub_sub_err)?,
        ))
        .map_err(map_embassy_spawn_err)?;

    Ok(())
}

#[embassy_executor::task]
async fn emitter(
    cfg: Config,
    i2c: I2C<'static, I2C0>,
    delay: Delay,
    publisher: Publisher<'static, CriticalSectionRawMutex, Option<SensorMetrics>, 1, 2, 1>,
) {
    let i2c_rc = RefCell::new(i2c);

    loop {
        let i2c = RefCellDevice::new(&i2c_rc);

        match Device::new(cfg.load().as_ref(), i2c, delay) {
            Ok(mut dev) => {
                let mut failures: u8 = 0;
                loop {
                    match emitter_poll(&cfg, &mut dev, &publisher, &mut failures).await {
                        Ok(reload) => {
                            if reload {
                                log::info!("Reloading sensor device");
                                break;
                            }
                        }
                        Err(e) => {
                            log::warn!("Sensor emitter poll failed: {:?}", e);
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to create sensor device: {:?}", e);
                publisher.publish_immediate(None);

                Timer::after(Duration::from_millis(cfg.load().sensor_delay_err_ms as u64)).await;
            }
        }
    }
}

async fn emitter_poll<'d>(
    cfg: &Config,
    dev: &mut Device<'d, I2C0>,
    publisher: &Publisher<'static, CriticalSectionRawMutex, Option<SensorMetrics>, 1, 2, 1>,
    failures: &mut u8,
) -> Result<bool> {
    let cfg = cfg.load();

    let msg = match dev.read() {
        Ok((temp, mut rh)) => {
            if temp > 0_f32 && rh > 0_f32 {
                if let Some(adj) = cfg.sensor_calibration_rh_adj {
                    rh += adj;
                    if rh > MAX_RH {
                        rh = MAX_RH;
                    }

                    log::debug!("Sensor - Temp: {}, RH: {}% (+{})", temp, rh, adj);
                } else {
                    log::debug!("Sensor - Temp: {}, RH: {}%", temp, rh);
                }

                Some(SensorMetrics { temp, rh })
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

    let is_ok = match msg.as_ref() {
        Some(metrics) => {
            let _ = METRICS.write().insert(metrics.clone());
            true
        }
        None => {
            let _ = METRICS.write().take();
            false
        }
    };

    publisher.publish_immediate(msg);

    if is_ok {
        Timer::after(Duration::from_millis(cfg.sensor_delay_ms as u64)).await;
    } else {
        if *failures >= 2 {
            // Re-create device.
            return Ok(true);
        }

        if let Err(e) = dev.reset() {
            log::error!("Failed to reset sensor: {:?}", e);
        }

        *failures += 1;

        Timer::after(Duration::from_millis(cfg.sensor_delay_err_ms as u64)).await;
    }

    Ok(false)
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SensorMetrics {
    pub(crate) temp: f32,
    pub(crate) rh: f32,
}

enum Device<'d, T> {
    #[cfg(feature = "hdc1080")]
    HDC1080(Hdc1080<RefCellDevice<'d, I2C<'d, T>>, Delay>),
    #[cfg(feature = "sht40")]
    SHT40(SHT40Driver<RefCellDevice<'d, I2C<'d, T>>, Delay>),
}

impl<'d, T> Device<'d, T>
where
    T: Instance,
{
    fn new(cfg: &ConfigInstance, i2c: RefCellDevice<'d, I2C<'d, T>>, delay: Delay) -> Result<Self> {
        log::info!("Creating sensor device driver for: {:?}", cfg.sensor_driver);

        match cfg.sensor_driver {
            #[cfg(feature = "hdc1080")]
            SensorDriver::HDC1080 => {
                let mut dev = Hdc1080::new(i2c, delay).map_err(|e| {
                    general_fault(format!("failed to create hdc1080 sensor device: {:?}", e))
                })?;

                dev.init().map_err(|e| {
                    general_fault(format!("failed to init hdc1080 sensor device: {:?}", e))
                })?;

                Ok(Device::HDC1080(dev))
            }
            #[cfg(feature = "sht40")]
            SensorDriver::SHT40 => Ok(Device::SHT40(SHT40Driver::new(
                i2c,
                I2CAddr::SHT4x_A,
                delay,
            ))),
        }
    }

    fn read(&mut self) -> Result<(f32, f32)> {
        match self {
            #[cfg(feature = "hdc1080")]
            Device::HDC1080(dev) => dev.read().map_err(|e| {
                general_fault(format!(
                    "failed to read from hdc1080 sensor device: {:?}",
                    e
                ))
            }),
            #[cfg(feature = "sht40")]
            Device::SHT40(dev) => {
                let measurement = dev
                    .get_temp_and_rh(Precision::High, TempUnit::MilliDegreesCelsius)
                    .map_err(|e| {
                        sensor_fault(format!("Failed to take measurement from sensor: {:?}", e))
                    })?;

                return Ok((
                    measurement.temp as f32 / 1000_f32,
                    measurement.rel_hum_pcm as f32 / 1000_f32,
                ));
            }
        }
    }

    #[allow(dead_code)]
    fn reset(&mut self) -> Result<()> {
        match self {
            #[cfg(feature = "hdc1080")]
            Device::HDC1080(dev) => dev.reset().map_err(|e| {
                general_fault(format!("failed to reset hdc1080 sensor device: {:?}", e))
            }),
            #[cfg(feature = "sht40")]
            Device::SHT40(dev) => dev.soft_reset_device().map_err(|e| {
                general_fault(format!("failed to reset sht40 sensor device: {:?}", e))
            }),
        }
    }
}
