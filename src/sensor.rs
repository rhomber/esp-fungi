use alloc::format;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embedded_hal::delay::DelayNs;

#[cfg(feature = "hdc1080")]
use embedded_hdc1080_rs::Hdc1080;
use esp_hal::clock::Clocks;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::i2c::{Instance, I2C};
use esp_hal::peripheral::Peripheral;
use esp_hal::peripherals::I2C0;
use esp_hal::Delay;
use esp_println::println;
use fugit::RateExtU32;

use crate::error::{general_fault, map_embassy_spawn_err, Result};

pub(crate) fn init<SDA, SDA_, SCL, SCL_>(
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

    spawner.spawn(emitter(dev)).map_err(map_embassy_spawn_err)?;

    Ok(())
}

#[embassy_executor::task]
async fn emitter(mut dev: Device<'static, I2C0>) {
    loop {
        match dev.read() {
            Ok((temp, hum)) => {
                println!("Temp: {}, Humidity: {}", temp, hum);

                Timer::after(Duration::from_millis(5_000)).await;
            }
            Err(e) => {
                log::error!("Failed to read from sensor: {:?}", e);

                Timer::after(Duration::from_millis(10_000)).await;
            }
        }
    }
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
