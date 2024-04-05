#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

mod error;
mod network;

extern crate alloc;
use core::mem::MaybeUninit;
use embassy_executor::Spawner;
use esp_backtrace as _;
use esp_hal::{clock::ClockControl, embassy, peripherals::Peripherals, prelude::*, Delay, Rtc};
use esp_println::println;

use esp_wifi::{initialize, EspWifiInitFor};

use esp_hal::{timer::TimerGroup, Rng};
#[global_allocator]
static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

fn init_heap() {
    const HEAP_SIZE: usize = 32 * 1024;
    static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();

    unsafe {
        ALLOCATOR.init(HEAP.as_mut_ptr() as *mut u8, HEAP_SIZE);
    }
}

#[main]
async fn main(spawner: Spawner) {
    init_heap();

    // setup logger
    // To change the log_level change the env section in .cargo/config.toml
    // or remove it and set ESP_LOGLEVEL manually before running cargo run
    // this requires a clean rebuild because of https://github.com/rust-lang/cargo/issues/10358
    esp_println::logger::init_logger_from_env();

    let peripherals = Peripherals::take();
    let system = peripherals.SYSTEM.split();

    let clocks = ClockControl::max(system.clock_control).freeze();
    let mut delay = Delay::new(&clocks);

    let timer_group0 = TimerGroup::new(peripherals.TIMG0, &clocks);
    let timer_group1 = TimerGroup::new(peripherals.TIMG1, &clocks);

    // Init embassy
    embassy::init(&clocks, timer_group0);

    // init network
    network::init(
        peripherals.WIFI,
        peripherals.RNG,
        timer_group1,
        system.radio_clock_control,
        &clocks,
    )
    .expect("failed to init network");

    loop {
        println!("Loop...");
        delay.delay_ms(500u32);
    }
}
