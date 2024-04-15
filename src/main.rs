#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

pub(crate) mod chip_control;
pub(crate) mod config;
mod controls;
mod display;
pub(crate) mod error;
mod mister;
mod network;
pub(crate) mod sensor;
pub(crate) mod utils;

extern crate alloc;

use core::mem::MaybeUninit;
use embassy_executor::Spawner;
use esp_backtrace as _;
use esp_hal::{clock::ClockControl, embassy, peripherals::Peripherals, prelude::*, IO};

use crate::config::Config;
use esp_hal::timer::TimerGroup;

#[global_allocator]
static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

fn init_heap() {
    const HEAP_SIZE: usize = 64 * 1024;
    static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();

    unsafe {
        ALLOCATOR.init(HEAP.as_mut_ptr() as *mut u8, HEAP_SIZE);
    }
}

#[main]
async fn main(spawner: Spawner) {
    init_heap();

    // static config
    let cfg = Config::new().expect("failed to load config");

    // setup logger
    // To change the log_level change the env section in .cargo/config.toml
    // or remove it and set ESP_LOGLEVEL manually before running cargo run
    // this requires a clean rebuild because of https://github.com/rust-lang/cargo/issues/10358
    esp_println::logger::init_logger_from_env();

    let peripherals = Peripherals::take();
    let system = peripherals.SYSTEM.split();
    let gpio = IO::new(peripherals.GPIO, peripherals.IO_MUX);

    if let Err(e) = esp_hal::interrupt::enable(
        esp_hal::peripherals::Interrupt::GPIO,
        esp_hal::interrupt::Priority::Priority1,
    ) {
        log::error!("Failed to enable esp hal interrupt: {:?}", e);
    }

    let clocks = ClockControl::max(system.clock_control).freeze();

    let timer_group0 = TimerGroup::new(peripherals.TIMG0, &clocks);
    let timer_group1 = TimerGroup::new(peripherals.TIMG1, &clocks);

    log::info!("main init: Started");

    // Init embassy
    embassy::init(&clocks, timer_group0);

    // Init chip control
    if let Err(e) = chip_control::init(cfg.clone(), &spawner) {
        log::error!("Failed to init chip control: {:?}", e);
    }

    if cfg.load().display_enabled {
        // Init display
        if let Err(e) = display::init(
            cfg.clone(),
            gpio.pins.gpio19,
            gpio.pins.gpio18,
            peripherals.I2C1,
            &clocks,
            &spawner,
        ) {
            log::error!("Failed to init display: {:?}", e);
        }
    }

    if cfg.load().network_enabled {
        // Init network
        if let Err(e) = network::init(
            cfg.clone(),
            peripherals.WIFI,
            peripherals.RNG,
            timer_group1,
            system.radio_clock_control,
            &clocks,
            &spawner,
        ) {
            log::error!("Failed to init network: {:?}", e);
        }
    }

    if cfg.load().sensor_enabled {
        // Init sensor
        if let Err(e) = sensor::init(
            cfg.clone(),
            gpio.pins.gpio14,
            gpio.pins.gpio15,
            peripherals.I2C0,
            &clocks,
            &spawner,
        ) {
            log::error!("Failed to init sensor: {:?}", e);
        }
    }

    // Init mister
    if let Err(e) = mister::init(cfg.clone(), gpio.pins.gpio17, gpio.pins.gpio22, &spawner) {
        log::error!("Failed to init mister: {:?}", e);
    }

    // Init controls
    if let Err(e) = controls::init(cfg.clone(), gpio.pins.gpio21, &spawner) {
        log::error!("Failed to init controls: {:?}", e);
    }

    log::info!("main init: Completed");
}
