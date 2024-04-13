use alloc::format;
use alloc::string::{String, ToString};
use embassy_executor::Spawner;
use embassy_futures::select::{select4, Either4};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pubsub::{PubSubChannel, Publisher, Subscriber, WaitResult};
use embassy_time::{Duration, Timer};
use embedded_graphics::mono_font::iso_8859_1::{FONT_10X20, FONT_6X12, FONT_8X13};
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
    display_draw_err, map_display_err, map_embassy_pub_sub_err, map_embassy_spawn_err, Result,
};
use crate::mister::{
    Mode as MisterMode, ModeChangedSubscriber as MisterModeChangedSubscriber,
    Status as MisterStatus, Status, StatusChangedSubscriber as MisterStatusChangedSubscriber,
};
use crate::network::wifi::IP_ADDRESS;
use crate::sensor::{SensorMetrics, SensorSubscriber};
use crate::{mister, sensor};

static DISPLAY_WIDTH: u32 = 128;
static DISPLAY_HALF_WIDTH: u32 = DISPLAY_WIDTH / 2;
static DISPLAY_HEIGHT: u32 = 64;

static GAUGE_LABEL_OFFSET_Y: i32 = 12;
static GAUGE_FONT_HEIGHT: u32 = 20;
static GAUGE_FONT_WIDTH: u32 = 10;
static GAUGE_PULL_SIDE_PX: u32 = 0;
static GAUGE_BOX_OFFSET_Y: i32 = 16;
static GAUGE_TEXT_OFFSET_Y: i32 = (GAUGE_BOX_OFFSET_Y + GAUGE_FONT_HEIGHT as i32) - 4;
static STATUS_BOX_HEIGHT: u32 = 24;
static STATUS_BOX_PADDING_X: u32 = 8;
static STATUS_BOX_PADDING_Y: u32 = 8;
static STATUS_FONT_WIDTH: u32 = 8;

type ChangeModeSubscriber = Subscriber<'static, CriticalSectionRawMutex, ChangeMode, 1, 1, 1>;
pub(crate) type ChangeModePublisher =
    Publisher<'static, CriticalSectionRawMutex, ChangeMode, 1, 1, 1>;
pub(crate) static CHANGE_MODE_CHANNEL: PubSubChannel<CriticalSectionRawMutex, ChangeMode, 1, 1, 1> =
    PubSubChannel::new();

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

    let mut display_renderer = DisplayRenderer::new(cfg.clone(), display, 0_f32, 0_f32);

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
            mister::STATUS_CHANGED_CHANNEL
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
    mut mister_status_changed_sub: MisterStatusChangedSubscriber,
) {
    loop {
        if let Err(e) = display_task_poll(
            &mut display_renderer,
            &mut change_mode_sub,
            &mut sensor_sub,
            &mut mister_mode_changed_sub,
            &mut mister_status_changed_sub,
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
    mister_status_changed_sub: &mut MisterStatusChangedSubscriber,
) -> Result<()> {
    match select4(
        sensor_sub.next_message(),
        change_mode_sub.next_message(),
        mister_mode_changed_sub.next_message(),
        mister_status_changed_sub.next_message(),
    )
    .await
    {
        Either4::First(r) => match r {
            WaitResult::Lagged(count) => {
                log::warn!("display sensor subscriber lagged by {} messages", count);

                // Ignore
                return Ok(());
            }
            WaitResult::Message(Some(msg)) => {
                display_renderer.apply_sensor_msg(msg);
            }
            WaitResult::Message(None) => {
                display_renderer.clear_sensor();
            }
        },
        Either4::Second(r) => match r {
            WaitResult::Lagged(count) => {
                log::warn!("display mode subscriber lagged by {} messages", count);

                // Ignore
                return Ok(());
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
        Either4::Third(r) => match r {
            WaitResult::Lagged(count) => {
                log::warn!("mister mode subscriber lagged by {} messages", count);

                // Ignore
                return Ok(());
            }
            WaitResult::Message(mode) => {
                display_renderer.mister_mode(Some(mode));
            }
        },
        Either4::Fourth(r) => match r {
            WaitResult::Lagged(count) => {
                log::warn!("mister status subscriber lagged by {} messages", count);

                // Ignore
                return Ok(());
            }
            WaitResult::Message(status) => {
                display_renderer.mister_status(status);
            }
        },
    }

    display_renderer.draw()
}

struct DisplayRenderer<'d> {
    cfg: Config,
    display: Ssd1306<
        I2CInterface<I2C<'d, I2C1>>,
        DisplaySize128x64,
        BufferedGraphicsMode<DisplaySize128x64>,
    >,
    bg_style: PrimitiveStyle<BinaryColor>,
    text_style: MonoTextStyle<'d, BinaryColor>,
    status_text_style: MonoTextStyle<'d, BinaryColor>,
    stale: bool,
    temp: f32,
    rh: f32,
    mode: Mode,
    mister_mode: Option<MisterMode>,
    mister_status: Status,
}

impl<'d> DisplayRenderer<'d> {
    fn new(
        cfg: Config,
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
        let status_text_style = MonoTextStyle::new(&FONT_8X13, BinaryColor::On);

        Self {
            cfg,
            display,
            bg_style,
            text_style,
            status_text_style,
            stale: true,
            temp,
            rh,
            mode: Mode::default(),
            mister_mode: None,
            mister_status: mister::STATUS.read().clone().unwrap_or(Status::Off),
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

        // Status Area
        Rectangle::new(
            Point::new(0, (DISPLAY_HEIGHT - STATUS_BOX_HEIGHT) as i32),
            Size::new(DISPLAY_WIDTH, STATUS_BOX_HEIGHT),
        )
        .into_styled(self.bg_style)
        .draw(&mut self.display)
        .map_err(|e| display_draw_err(format!("{:?}", e)))?;

        match self.mode {
            Mode::MisterMode => match self.mister_mode {
                Some(MisterMode::Auto) => {
                    let text = match mister::ACTIVE_AUTO
                        .read()
                        .get_auto_schedule(self.cfg.load().as_ref())
                        .clone()
                    {
                        Some((rh, _)) => format!("AUTO {}%", rh.ceil() as u32),
                        None => "AUTO ??%".to_string(),
                    };

                    self.draw_general_status(text)?;
                    self.draw_mister_status(self.mister_status)?;
                }
                Some(MisterMode::On) => self.draw_mister_status(MisterStatus::On)?,
                Some(MisterMode::Off) => self.draw_mister_status(MisterStatus::Off)?,
                None => {}
            },
            Mode::Info => {
                self.draw_info()?;
            }
        }

        self.display.flush().map_err(map_display_err)?;

        Ok(())
    }

    fn draw_general_status(&mut self, text: String) -> Result<()> {
        let x_offset = if text.len() >= DISPLAY_HALF_WIDTH as usize {
            (DISPLAY_WIDTH - (text.len() as u32 * STATUS_FONT_WIDTH)) / 2
        } else {
            STATUS_BOX_PADDING_X
        };

        Text::new(
            text.as_str(),
            Point::new(
                x_offset as i32,
                (DISPLAY_HEIGHT - STATUS_BOX_PADDING_Y) as i32,
            ),
            self.status_text_style,
        )
        .draw(&mut self.display)
        .map_err(|e| display_draw_err(format!("{:?}", e)))?;

        Ok(())
    }

    fn draw_mister_status(&mut self, status: MisterStatus) -> Result<()> {
        let text = match status {
            MisterStatus::On => "ON",
            MisterStatus::Off => "OFF",
            MisterStatus::Fault => "FAULT",
        };

        Text::with_alignment(
            text,
            Point::new(
                (DISPLAY_WIDTH - STATUS_BOX_PADDING_X) as i32,
                (DISPLAY_HEIGHT - STATUS_BOX_PADDING_Y) as i32,
            ),
            self.status_text_style,
            Alignment::Right,
        )
        .draw(&mut self.display)
        .map_err(|e| display_draw_err(format!("{:?}", e)))?;

        Ok(())
    }

    fn draw_info(&mut self) -> Result<()> {
        let ip = match IP_ADDRESS.read().as_ref() {
            Some(ip) => ip.to_string(),
            None => "NO WIFI".to_string(),
        };

        self.draw_general_status(ip)
    }

    // Accessors

    fn mode(&mut self, val: Mode) {
        self.mode = val;
        self.stale = true
    }

    fn mister_mode(&mut self, val: Option<MisterMode>) {
        self.mister_mode = val;
        self.stale = true
    }

    fn mister_status(&mut self, val: MisterStatus) {
        self.mister_status = val;
        self.stale = true
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

#[derive(Copy, Clone, PartialEq, Debug)]
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
