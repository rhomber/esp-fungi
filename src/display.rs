use alloc::format;
use alloc::string::ToString;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pubsub::{Subscriber, WaitResult};
use embassy_time::{Duration, Timer};

use crate::config::Config;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::{Alignment, Text};
use esp_hal::clock::Clocks;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::i2c::I2C;
use esp_hal::peripheral::Peripheral;
use esp_hal::peripherals::I2C1;
use esp_hal::{prelude::*, spi::master::prelude::*, Delay};
use esp_println::println;
use fugit::RateExtU32;
use ssd1306::prelude::*;
use ssd1306::{I2CDisplayInterface, Ssd1306};
use ssd1306::mode::{BasicMode, BufferedGraphicsMode};
use u8g2_fonts::{FontRenderer, fonts};
use u8g2_fonts::types::{FontColor, HorizontalAlignment, VerticalPosition};

use crate::error::{display_draw_err, map_display_err, map_embassy_pub_sub_err, map_embassy_spawn_err, Result};
use crate::sensor;
use crate::sensor::{ChannelMessage, SensorSubscriber};

pub(crate) fn init<SDA, SCL>(
    cfg: Config,
    sda: impl Peripheral<P = SDA> + 'static,
    scl: impl Peripheral<P = SCL> + 'static,
    i2c1: I2C1,
    clocks: &Clocks,
    spawner: &Spawner,
) -> Result<()>
where
    SDA: InputPin + OutputPin,
    SCL: InputPin + OutputPin,
{
    let mut delay = Delay::new(&clocks);

    let i2c = I2C::new(i2c1, sda, scl, 400_u32.kHz(), &clocks);

    let interface = I2CDisplayInterface::new(i2c);

    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();

    display.init().map_err(map_display_err)?;

    log::info!("Initialized display");

    display.flush().map_err(map_display_err)?;

    let mut display_renderer = DisplayRenderer::new(display, 0_f32, 0_f32);

    // Initial draw
    display_renderer.draw()?;

    log::info!("Drew initial display");

    /*

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

     */

    let sensor_subscriber = sensor::CHANNEL
        .subscriber()
        .map_err(map_embassy_pub_sub_err)?;

    spawner
        .spawn(simples(display_renderer, sensor_subscriber))
        .map_err(map_embassy_spawn_err)?;

    Ok(())
}

#[embassy_executor::task]
async fn simples(mut display_renderer: DisplayRenderer<'static>, mut sensor_subscriber: SensorSubscriber) {
    loop {
        match sensor_subscriber.next_message().await {
            WaitResult::Lagged(count) => {
                log::warn!("display sensor subscriber lagged by {} messages", count);

                // Some sleep to avoid thrashing.
                Timer::after(Duration::from_millis(50)).await;
                continue;
            }
            WaitResult::Message(Some(msg)) => {
                display_renderer.apply_sensor_msg(msg);
            }
            WaitResult::Message(None) => {
                display_renderer.clear_sensor();
            }
        }

        if let Err(e) = display_renderer.draw() {
            log::warn!("Failed to draw display: {:?}", e);
        }
    }
}

struct DisplayRenderer<'d> {
    display: Ssd1306<I2CInterface<I2C<'d, I2C1>>, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>>,
    bg_style: PrimitiveStyle<BinaryColor>,
    font: FontRenderer,
    stale: bool,
    temp: f32,
    rh: f32
}

impl<'d> DisplayRenderer<'d> {
    fn new(display: Ssd1306<I2CInterface<I2C<'d, I2C1>>, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>>, temp: f32, rh: f32) -> Self {
        let bg_style = PrimitiveStyleBuilder::new()
            .stroke_color(BinaryColor::On)
            .stroke_width(1)
            .fill_color(BinaryColor::Off)
            .build();

        let font = FontRenderer::new::<fonts::u8g2_font_haxrcorp4089_t_cyrillic>();

        Self { display, bg_style, font, stale: true, temp, rh }
    }

    fn apply_sensor_msg(&mut self, msg: ChannelMessage) {
        self.temp(msg.temp);
        self.rh(msg.rh);
    }

    fn clear_sensor(&mut self) {
        self.temp(0_f32);
        self.rh(0_f32);
    }

    fn draw(&mut self) -> Result<()> {
        if !self.stale {
            return Ok(());
        }

        // temp
        Rectangle::new(Point::new(0, 18), Size::new(64, 28))
            .into_styled(self.bg_style)
            .draw(&mut self.display)
            .map_err(|e| display_draw_err(format!("{:?}", e)))?;

        self.font.render_aligned(
            "30",
            self.display.bounding_box().center() + Point::new(0, 16),
            VerticalPosition::Baseline,
            HorizontalAlignment::Center,
            FontColor::Transparent(BinaryColor::On),
            &mut self.display,
        ).map_err(|e| display_draw_err(format!("{:?}", e)))?;

        /*
        Text::new(
            "30°C",
            Point::new(3, 44),
            self.text_style,
        )
            .draw(&mut self.display)
            .map_err(|e| display_draw_err(format!("{:?}", e)))?;

         */

        // RH
        Rectangle::new(Point::new(64, 18), Size::new(64, 28))
            .into_styled(self.bg_style)
            .draw(&mut self.display)
            .map_err(|e| display_draw_err(format!("{:?}", e)))?;

        /*
        Text::new(
            "77%",
            Point::new(70, 44),
            self.text_style,
        )
            .draw(&mut self.display)
            .map_err(|e| display_draw_err(format!("{:?}", e)));
         */

        self.display.flush().map_err(map_display_err)?;

        Ok(())
    }

    // Accessors

    fn temp(&mut self, val: f32) {
        if val != self.temp {
            self.temp = val;
            self.stale = true
        }
    }

    fn rh(&mut self, val: f32) {
        if val != self.rh {
            self.rh = val;
            self.stale = true
        }
    }
}