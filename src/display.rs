use alloc::format;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::{Alignment, Text};
use esp_hal::clock::Clocks;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::i2c::I2C;
use esp_hal::peripheral::Peripheral;
use esp_hal::peripherals::I2C0;
use esp_hal::{prelude::*, spi::master::prelude::*, Delay};
use esp_println::println;
use fugit::RateExtU32;
use profont::PROFONT_7_POINT;
use ssd1306::prelude::*;
use ssd1306::{I2CDisplayInterface, Ssd1306};

use crate::error::Result;

pub(crate) fn init<SDA, SDA_, SCL, SCL_>(
    sda: SDA,
    scl: SCL,
    i2c0: I2C0,
    clocks: &Clocks,
    spawner: &Spawner
) -> Result<()>
where
    SDA: Peripheral<P = SDA_>,
    SDA_: InputPin + OutputPin,
    SCL: Peripheral<P = SCL_>,
    SCL_: InputPin + OutputPin,
    SCL: Peripheral<P = SCL_>,
{
    let mut delay = Delay::new(&clocks);

    println!("init_display: begin");

    let i2c = I2C::new(i2c0, sda, scl, 400_u32.kHz(), &clocks);

    println!("init_display: opened GPIO");

    let interface = I2CDisplayInterface::new(i2c);

    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();

    println!("init_display: constructed display");

    display.init().unwrap();

    println!("init_display: init display");

    display.flush().unwrap();

    println!("init_display: turned on display");

    // TESTING:

    let style = PrimitiveStyleBuilder::new()
        .stroke_color(BinaryColor::On)
        .stroke_width(1)
        .fill_color(BinaryColor::Off)
        .build();

    let inv_style = PrimitiveStyleBuilder::new()
        .stroke_color(BinaryColor::On)
        .stroke_width(1)
        .fill_color(BinaryColor::On)
        .build();

    let character_style = MonoTextStyle::new(&PROFONT_7_POINT, BinaryColor::On);

    let inv_character_style = MonoTextStyle::new(&PROFONT_7_POINT, BinaryColor::Off);

    Rectangle::new(Point::new(0, 0), Size::new(128, 64))
        .into_styled(style)
        .draw(&mut display)
        .unwrap();

    Text::with_alignment(
        "Countulator Ha Ha Ha",
        Point::new(10, 12),
        character_style,
        Alignment::Left,
    )
    .draw(&mut display)
    .unwrap();

    Rectangle::new(Point::new(0, 43), Size::new(128, 21))
        .into_styled(style)
        .draw(&mut display)
        .unwrap();

    Text::with_alignment(
        "Count",
        Point::new(10, 55),
        character_style,
        Alignment::Left,
    )
    .draw(&mut display)
    .unwrap();

    for count in 1..3 {
        Rectangle::new(Point::new(64, 43), Size::new(64, 21))
            .into_styled(inv_style)
            .draw(&mut display)
            .unwrap();

        Text::with_alignment(
            format!("{}", count).as_str(),
            Point::new(70, 55),
            inv_character_style,
            Alignment::Left,
        )
        .draw(&mut display)
        .unwrap();

        display.flush().unwrap();
        delay.delay_ms(5_u32);
    }

    spawner.spawn(simples()).unwrap();

    Ok(())
}

#[embassy_executor::task]
async fn simples() {
    loop {
        println!("Hello world from embassy using esp-hal-async!");
        log::info!("simples loopy");
        Timer::after(Duration::from_millis(1_000)).await;
    }
}
