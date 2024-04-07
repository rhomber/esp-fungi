use alloc::format;
use embassy_executor::Spawner;
use embassy_futures::select::{select3, Either3};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pubsub::{PubSubChannel, Publisher, Subscriber, WaitResult};
use embassy_time::{Duration, Timer};
use embedded_graphics::mono_font::iso_8859_1::{FONT_10X20, FONT_6X12};
use num_traits::float::Float;

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
use fugit::RateExtU32;
use ssd1306::mode::BufferedGraphicsMode;
use ssd1306::prelude::*;
use ssd1306::{I2CDisplayInterface, Ssd1306};

use crate::error::{
    display_draw_err, general_fault, map_display_err, map_embassy_pub_sub_err,
    map_embassy_spawn_err, Result,
};
use crate::mister::{Mode as MisterMode, ModeChangedSubscriber as MisterModeChangedSubscriber};
use crate::sensor::{SensorMetrics, SensorSubscriber};
use crate::{mister, sensor};

static DISPLAY_WIDTH: u32 = 128;
static DISPLAY_HALF_WIDTH: u32 = DISPLAY_WIDTH / 2;

static GAUGE_LABEL_OFFSET_Y: i32 = 12;
static GAUGE_FONT_HEIGHT: u32 = 20;
static GAUGE_FONT_WIDTH: u32 = 10;
static GAUGE_PULL_SIDE_PX: u32 = 0;
static GAUGE_BOX_OFFSET_Y: i32 = 16;
static GAUGE_TEXT_OFFSET_Y: i32 = (GAUGE_BOX_OFFSET_Y + GAUGE_FONT_HEIGHT as i32) - 4;

type ChangeModeSubscriber = Subscriber<'static, CriticalSectionRawMutex, ChangeMode, 1, 1, 1>;
pub(crate) type ChangeModePublisher =
    Publisher<'static, CriticalSectionRawMutex, ChangeMode, 1, 1, 1>;
pub(crate) static CHANGE_MODE_CHANNEL: PubSubChannel<CriticalSectionRawMutex, ChangeMode, 1, 1, 1> =
    PubSubChannel::new();

pub(crate) fn init<SDA, SCL>(
    _cfg: Config,
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
    let i2c = I2C::new(i2c1, sda, scl, 400_u32.kHz(), &clocks);

    let interface = I2CDisplayInterface::new(i2c);

    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();

    display.init().map_err(map_display_err)?;

    log::info!("Initialized display");

    let label_text_style = MonoTextStyle::new(&FONT_6X12, BinaryColor::On);

    Text::new(
        "TEMP",
        Point::new(calculate_gauge_x(4, 6, 0), GAUGE_LABEL_OFFSET_Y),
        label_text_style,
    )
    .draw(&mut display)
    .map_err(|e| display_draw_err(format!("{:?}", e)))?;

    Text::with_alignment(
        "RH",
        Point::new(
            DISPLAY_WIDTH as i32 - calculate_gauge_x(2, 6, 0),
            GAUGE_LABEL_OFFSET_Y,
        ),
        label_text_style,
        Alignment::Right,
    )
    .draw(&mut display)
    .map_err(|e| display_draw_err(format!("{:?}", e)))?;

    display.flush().map_err(map_display_err)?;

    let mut display_renderer = DisplayRenderer::new(display, 0_f32, 0_f32);

    // Initial draw
    display_renderer.draw()?;

    log::info!("Drew initial display");

    spawner
        .spawn(display_task(
            display_renderer,
            CHANGE_MODE_CHANNEL
                .subscriber()
                .map_err(map_embassy_pub_sub_err)?,
            sensor::CHANNEL
                .subscriber()
                .map_err(map_embassy_pub_sub_err)?,
            mister::MODE_CHANGED_CHANNEL
                .subscriber()
                .map_err(map_embassy_pub_sub_err)?,
        ))
        .map_err(map_embassy_spawn_err)?;

    Ok(())
}

#[embassy_executor::task]
async fn display_task(
    mut display_renderer: DisplayRenderer<'static>,
    mut change_mode_sub: ChangeModeSubscriber,
    mut sensor_sub: SensorSubscriber,
    mut mister_mode_changed_sub: MisterModeChangedSubscriber,
) {
    loop {
        if let Err(e) = display_task_poll(
            &mut display_renderer,
            &mut change_mode_sub,
            &mut sensor_sub,
            &mut mister_mode_changed_sub,
        )
        .await
        {
            log::warn!("Failed to run display task poll: {:?}", e);

            // Some sleep to avoid thrashing.
            Timer::after(Duration::from_millis(50)).await;
            continue;
        }
    }
}

async fn display_task_poll(
    display_renderer: &mut DisplayRenderer<'static>,
    change_mode_sub: &mut ChangeModeSubscriber,
    sensor_sub: &mut SensorSubscriber,
    mister_mode_changed_sub: &mut MisterModeChangedSubscriber,
) -> Result<()> {
    match select3(
        sensor_sub.next_message(),
        change_mode_sub.next_message(),
        mister_mode_changed_sub.next_message(),
    )
    .await
    {
        Either3::First(r) => match r {
            WaitResult::Lagged(count) => {
                return Err(general_fault(format!(
                    "display sensor subscriber lagged by {} messages",
                    count
                )));
            }
            WaitResult::Message(Some(msg)) => {
                display_renderer.apply_sensor_msg(msg);
            }
            WaitResult::Message(None) => {
                display_renderer.clear_sensor();
            }
        },
        Either3::Second(r) => match r {
            WaitResult::Lagged(count) => {
                return Err(general_fault(format!(
                    "display mode subscriber lagged by {} messages",
                    count
                )));
            }
            WaitResult::Message(change_mode) => match change_mode.mode {
                Some(mode) => {
                    display_renderer.mode(mode);
                }
                None => {
                    display_renderer.mode(Mode::default());
                }
            },
        },
        Either3::Third(r) => match r {
            WaitResult::Lagged(count) => {
                return Err(general_fault(format!(
                    "mister mode subscriber lagged by {} messages",
                    count
                )));
            }
            WaitResult::Message(mode) => {
                display_renderer.mister_mode(Some(mode));
            }
        },
    }

    display_renderer.draw()
}

struct DisplayRenderer<'d> {
    display: Ssd1306<
        I2CInterface<I2C<'d, I2C1>>,
        DisplaySize128x64,
        BufferedGraphicsMode<DisplaySize128x64>,
    >,
    bg_style: PrimitiveStyle<BinaryColor>,
    text_style: MonoTextStyle<'d, BinaryColor>,
    stale: bool,
    temp: f32,
    rh: f32,
    mode: Mode,
    mister_mode: Option<MisterMode>,
}

impl<'d> DisplayRenderer<'d> {
    fn new(
        display: Ssd1306<
            I2CInterface<I2C<'d, I2C1>>,
            DisplaySize128x64,
            BufferedGraphicsMode<DisplaySize128x64>,
        >,
        temp: f32,
        rh: f32,
    ) -> Self {
        let bg_style = PrimitiveStyleBuilder::new()
            .stroke_color(BinaryColor::Off)
            .stroke_width(1)
            .fill_color(BinaryColor::Off)
            .build();

        let text_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);

        Self {
            display,
            bg_style,
            text_style,
            stale: true,
            temp,
            rh,
            mode: Mode::default(),
            mister_mode: None,
        }
    }

    fn apply_sensor_msg(&mut self, msg: SensorMetrics) {
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
        self.stale = false;

        // Temp
        Rectangle::new(
            Point::new(0, GAUGE_BOX_OFFSET_Y),
            Size::new(DISPLAY_HALF_WIDTH, GAUGE_FONT_HEIGHT),
        )
        .into_styled(self.bg_style)
        .draw(&mut self.display)
        .map_err(|e| display_draw_err(format!("{:?}", e)))?;

        let temp = self.temp.ceil() as u32;

        Text::new(
            format!("{}°C", temp).as_str(),
            Point::new(
                calculate_gauge_x(
                    if temp >= 10 { 4 } else { 3 },
                    GAUGE_FONT_WIDTH,
                    GAUGE_PULL_SIDE_PX,
                ),
                GAUGE_TEXT_OFFSET_Y,
            ),
            self.text_style,
        )
        .draw(&mut self.display)
        .map_err(|e| display_draw_err(format!("{:?}", e)))?;

        // RH
        Rectangle::new(
            Point::new(DISPLAY_HALF_WIDTH as i32, GAUGE_BOX_OFFSET_Y),
            Size::new(DISPLAY_HALF_WIDTH, GAUGE_FONT_HEIGHT),
        )
        .into_styled(self.bg_style)
        .draw(&mut self.display)
        .map_err(|e| display_draw_err(format!("{:?}", e)))?;

        Text::with_alignment(
            format!("{:.1}%", self.rh).as_str(),
            Point::new(
                DISPLAY_WIDTH as i32
                    - calculate_gauge_x(
                        if self.rh >= 10_f32 { 5 } else { 4 },
                        GAUGE_FONT_WIDTH,
                        GAUGE_PULL_SIDE_PX,
                    ),
                GAUGE_TEXT_OFFSET_Y,
            ),
            self.text_style,
            Alignment::Right,
        )
        .draw(&mut self.display)
        .map_err(|e| display_draw_err(format!("{:?}", e)))?;

        self.display.flush().map_err(map_display_err)?;

        Ok(())
    }

    // Accessors

    fn mode(&mut self, val: Mode) {
        if val != self.mode {
            self.mode = val;
            self.stale = true
        }
    }

    fn mister_mode(&mut self, val: Option<MisterMode>) {
        if val != self.mister_mode {
            self.mister_mode = val;
            self.stale = true
        }
    }

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

// Models

#[derive(Copy, Clone, PartialEq)]
pub(crate) enum Mode {
    MisterMode,
    Info,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::MisterMode
    }
}

#[derive(Copy, Clone)]
pub(crate) struct ChangeMode {
    mode: Option<Mode>,
}

impl ChangeMode {
    pub(crate) fn new(mode: Option<Mode>) -> Self {
        Self { mode }
    }
}

impl Default for ChangeMode {
    fn default() -> Self {
        Self::new(None)
    }
}

// Utils

fn calculate_gauge_x(chars: u32, font_width: u32, pull_side_px: u32) -> i32 {
    let mut x = (((DISPLAY_HALF_WIDTH - (chars * font_width)) / 2) - pull_side_px) as i32;
    if x < 0 {
        x = 0;
    }

    x
}
