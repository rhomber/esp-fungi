use alloc::format;
use alloc::string::ToString;
use alloc::sync::Arc;
use core::fmt::{Display, Formatter};

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pubsub::{PubSubChannel, Publisher, Subscriber, WaitResult};
use embassy_time::{Duration, Timer};
use embedded_hal::digital::{OutputPin, StatefulOutputPin};
use embedded_storage::{ReadStorage, Storage};
use esp_hal::gpio::{GpioPin, Output, PushPull, Unknown};
use esp_storage::FlashStorage;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use spin::RwLock;

use crate::config::{Config, ConfigInstance};
use crate::error::{
    general_fault, map_embassy_pub_sub_err, map_embassy_spawn_err, map_infallible_err, Result,
};
use crate::sensor;
use crate::sensor::{SensorMetrics, SensorSubscriber};
use crate::utils::get_time_ms;

const MISTER_POWER_GPIO_PIN: u8 = 17;
const STATUS_LED_GPIO_PIN: u8 = 22;
const MODE_FLASH_ADDR: u32 = 0x9000;

type ChangeModeSubscriber = Subscriber<'static, CriticalSectionRawMutex, ChangeMode, 1, 2, 2>;
pub(crate) type ChangeModePublisher =
    Publisher<'static, CriticalSectionRawMutex, ChangeMode, 1, 2, 2>;
pub(crate) static CHANGE_MODE_CHANNEL: PubSubChannel<CriticalSectionRawMutex, ChangeMode, 1, 2, 2> =
    PubSubChannel::new();

type ModeChangedPublisher = Publisher<'static, CriticalSectionRawMutex, Mode, 1, 2, 1>;
pub(crate) type ModeChangedSubscriber = Subscriber<'static, CriticalSectionRawMutex, Mode, 1, 2, 1>;
pub(crate) static MODE_CHANGED_CHANNEL: PubSubChannel<CriticalSectionRawMutex, Mode, 1, 2, 1> =
    PubSubChannel::new();

pub(crate) static ACTIVE_MODE: RwLock<Option<Mode>> = RwLock::new(None);

pub(crate) type StatusChangedPublisher =
    Publisher<'static, CriticalSectionRawMutex, Status, 1, 2, 1>;
pub(crate) type StatusChangedSubscriber =
    Subscriber<'static, CriticalSectionRawMutex, Status, 1, 2, 1>;
pub(crate) static STATUS_CHANGED_CHANNEL: PubSubChannel<CriticalSectionRawMutex, Status, 1, 2, 1> =
    PubSubChannel::new();
pub(crate) static STATUS: RwLock<Option<Status>> = RwLock::new(Some(Status::Off));

pub(crate) static ACTIVE_AUTO: Lazy<RwLock<AutoScheduleState>> =
    Lazy::new(|| RwLock::new(AutoScheduleState::default()));

static AUTO_SCHEDULE_PENDING_SLEEP_MS: u32 = 100;

pub(crate) fn init(
    cfg: Config,
    mister_pwr_pin: GpioPin<Unknown, MISTER_POWER_GPIO_PIN>,
    status_led_pin: GpioPin<Unknown, STATUS_LED_GPIO_PIN>,
    spawner: &Spawner,
) -> Result<()> {
    spawner
        .spawn(mister_operation_task(
            cfg.clone(),
            mister_pwr_pin,
            MODE_CHANGED_CHANNEL
                .publisher()
                .map_err(map_embassy_pub_sub_err)?,
            CHANGE_MODE_CHANNEL
                .subscriber()
                .map_err(map_embassy_pub_sub_err)?,
            STATUS_CHANGED_CHANNEL
                .publisher()
                .map_err(map_embassy_pub_sub_err)?,
            sensor::CHANNEL
                .subscriber()
                .map_err(map_embassy_pub_sub_err)?,
        ))
        .map_err(map_embassy_spawn_err)?;

    spawner
        .spawn(mister_status_led_task(
            cfg.clone(),
            status_led_pin,
            STATUS_CHANGED_CHANNEL
                .subscriber()
                .map_err(map_embassy_pub_sub_err)?,
        ))
        .map_err(map_embassy_spawn_err)?;

    spawner
        .spawn(mister_auto_schedule_task(
            cfg.clone(),
            MODE_CHANGED_CHANNEL
                .subscriber()
                .map_err(map_embassy_pub_sub_err)?,
        ))
        .map_err(map_embassy_spawn_err)?;

    Ok(())
}

#[embassy_executor::task]
async fn mister_operation_task(
    cfg: Config,
    mister_pwr_pin: GpioPin<Unknown, MISTER_POWER_GPIO_PIN>,
    mut mode_changed_pub: ModeChangedPublisher,
    mut change_mode_sub: ChangeModeSubscriber,
    mut status_changed_pub: StatusChangedPublisher,
    mut sensor_sub: SensorSubscriber,
) {
    let mut storage = FlashStorage::new();
    load_mode(&mut storage, &mut mode_changed_pub).await;

    let mut mister_pwr_pin = mister_pwr_pin.into_push_pull_output();

    let mut auto_state: Option<AutoRhState> = None;

    loop {
        if let Err(e) = mister_operation_task_poll(
            cfg.load(),
            &mut storage,
            &mut mister_pwr_pin,
            &mut mode_changed_pub,
            &mut change_mode_sub,
            &mut status_changed_pub,
            &mut sensor_sub,
            &mut auto_state,
        )
        .await
        {
            log::warn!("mister operation task poll failed: {:?}", e);

            // Some sleep to avoid thrashing.
            Timer::after(Duration::from_millis(5000)).await;
            continue;
        }
    }
}

async fn mister_operation_task_poll(
    cfg: Arc<ConfigInstance>,
    storage: &mut FlashStorage,
    mister_pwr_pin: &mut GpioPin<Output<PushPull>, MISTER_POWER_GPIO_PIN>,
    mode_changed_pub: &mut ModeChangedPublisher,
    change_mode_sub: &mut ChangeModeSubscriber,
    status_changed_pub: &mut StatusChangedPublisher,
    sensor_sub: &mut SensorSubscriber,
    auto_state: &mut Option<AutoRhState>,
) -> Result<()> {
    match select(change_mode_sub.next_message(), sensor_sub.next_message()).await {
        Either::First(r) => match r {
            WaitResult::Lagged(count) => {
                log::warn!("mister mode subscriber lagged by {} messages", count);

                // Ignore
                return Ok(());
            }
            WaitResult::Message(change_mode) => match change_mode.mode {
                Some(mode) => {
                    store_mode(storage, mode, mode_changed_pub).await?;
                    change_status_from_mode(mode, mister_pwr_pin, status_changed_pub).await?;
                }
                None => {
                    let mode = toggle_mode(storage, mode_changed_pub).await?;
                    change_status_from_mode(mode, mister_pwr_pin, status_changed_pub).await?;
                }
            },
        },
        Either::Second(r) => {
            if is_mode_auto() {
                match r {
                    WaitResult::Lagged(count) => {
                        log::warn!("sensor subscriber lagged by {} messages", count);

                        // Ignore
                        return Ok(());
                    }
                    WaitResult::Message(metrics) => {
                        match ACTIVE_AUTO.read().get_auto_schedule(cfg.as_ref()) {
                            Some((target_rh, _)) => {
                                mister_auto_rh_poll(
                                    cfg,
                                    auto_state,
                                    target_rh,
                                    metrics,
                                    mister_pwr_pin,
                                    status_changed_pub,
                                )
                                .await?;
                            }
                            None => {
                                change_status(Status::Fault, mister_pwr_pin, status_changed_pub)
                                    .await?;

                                // Clear state.
                                let _ = auto_state.take();

                                return Err(general_fault(
                                    "mister mode is auto without valid schedule present"
                                        .to_string(),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

struct AutoRhState {
    status: Status,
    cycle_start_time: u32,
}

impl AutoRhState {
    fn new(status: Status, cycle_start_time: u32) -> Self {
        Self {
            status,
            cycle_start_time,
        }
    }
}

async fn mister_auto_rh_poll(
    cfg: Arc<ConfigInstance>,
    state: &mut Option<AutoRhState>,
    target_rh: f32,
    metrics: Option<SensorMetrics>,
    mister_pwr_pin: &mut GpioPin<Output<PushPull>, MISTER_POWER_GPIO_PIN>,
    status_changed_pub: &mut StatusChangedPublisher,
) -> Result<()> {
    match metrics {
        Some(metrics) => {
            let status = STATUS.read().clone();
            let rh_on = cfg.mister_auto_on_rh(target_rh);
            let rh_off = target_rh;

            // Verify state is accurate.
            if let Some(cur) = state.as_ref() {
                if let Some(status) = status.as_ref() {
                    if !cur.status.eq(status) {
                        // Clear state.
                        let _ = state.take();
                    }
                }
            }

            // Determine new status
            let new_status = if metrics.rh <= rh_on {
                Status::On
            } else if metrics.rh >= rh_off {
                Status::Off
            } else {
                // If rh between on and off threshold preserve status (either 'rising' or 'falling').
                status.clone().unwrap_or(Status::Off)
            };

            // Change status with guarding against flapping too fast
            if let Some(status) = status.as_ref() {
                if !new_status.eq(status) {
                    match state.take() {
                        Some(mut cur) => {
                            // Check threshold and ignore event if required.
                            if (get_time_ms() - cur.cycle_start_time)
                                >= cfg.mister_auto_duration_min_ms
                            {
                                cur.cycle_start_time = get_time_ms();

                                change_status(new_status, mister_pwr_pin, status_changed_pub)
                                    .await?;
                            }

                            let _ = state.insert(cur);

                            Ok(())
                        }
                        None => {
                            let _ = state.insert(AutoRhState::new(new_status, get_time_ms()));
                            change_status(new_status, mister_pwr_pin, status_changed_pub).await
                        }
                    }
                } else {
                    // This just verifies pin state.
                    change_status(new_status, mister_pwr_pin, status_changed_pub).await
                }
            } else {
                // Assume first init (shouldn't ever be None here though).

                // Clear state.
                let _ = state.take();

                change_status(new_status, mister_pwr_pin, status_changed_pub).await
            }
        }
        None => {
            log::warn!("No metrics returned by sensor, setting mister status to 'Fault'");

            // Clear state.
            let _ = state.take();

            change_status(Status::Fault, mister_pwr_pin, status_changed_pub).await
        }
    }
}

#[derive(Clone, Serialize)]
pub(crate) enum AutoScheduleMode {
    Initial,
    Pending,
    Running,
}

#[derive(Clone)]
pub(crate) struct AutoScheduleState {
    pub(crate) mode: AutoScheduleMode,
    pub(crate) idx: usize,
    pub(crate) start_time: u32,
}

impl AutoScheduleState {
    fn new(mode: AutoScheduleMode, idx: usize, start_time: u32) -> Self {
        Self {
            mode,
            idx,
            start_time,
        }
    }

    fn reset(&mut self) {
        self.mode = AutoScheduleMode::Initial;
        self.idx = 0;
        self.start_time = 0;
    }

    pub(crate) fn running_ms(&self) -> u32 {
        get_time_ms() - self.start_time
    }

    pub(crate) fn remaining_ms(&self, cfg: &ConfigInstance) -> Option<u32> {
        match self.get_auto_schedule(cfg) {
            Some((_rh, run_secs)) => Some((run_secs * 1000) - self.running_ms()),
            None => None,
        }
    }
    pub(crate) fn get_auto_schedule(&self, cfg: &ConfigInstance) -> Option<(f32, u32)> {
        cfg.mister_auto_rh_schedule.get(self.idx).cloned()
    }
}

impl Default for AutoScheduleState {
    fn default() -> Self {
        Self::new(AutoScheduleMode::Initial, 0, 0)
    }
}

#[embassy_executor::task]
async fn mister_auto_schedule_task(cfg: Config, mut mode_changed_sub: ModeChangedSubscriber) {
    loop {
        match mister_auto_schedule_task_poll(cfg.load(), &mut mode_changed_sub).await {
            Ok(_) => {
                // Yield.
                Timer::after(Duration::from_millis(50)).await;
            }
            Err(e) => {
                log::warn!("mister auto schedule task poll failed: {:?}", e);

                // Some sleep to avoid thrashing.
                Timer::after(Duration::from_millis(500)).await;
                continue;
            }
        }
    }
}

async fn mister_auto_schedule_task_poll(
    cfg: Arc<ConfigInstance>,
    mode_changed_sub: &mut ModeChangedSubscriber,
) -> Result<()> {
    // Init
    if matches!(ACTIVE_AUTO.read().mode, AutoScheduleMode::Initial) {
        if !is_mode_auto() {
            return Ok(());
        }

        // Initialize.
        mister_auto_schedule_start(cfg.as_ref(), 0).await?;
    } else if !is_mode_auto() {
        ACTIVE_AUTO.write().reset();
        return Ok(());
    }

    // Main
    let (_, schedule_sleep_secs) = get_auto_schedule(cfg.as_ref())?;

    let sleep_ms = match ACTIVE_AUTO.read().mode {
        AutoScheduleMode::Pending => AUTO_SCHEDULE_PENDING_SLEEP_MS,
        AutoScheduleMode::Running => {
            if ACTIVE_AUTO.read().start_time > 0 {
                (schedule_sleep_secs * 1000) - ACTIVE_AUTO.read().running_ms()
            } else {
                ACTIVE_AUTO.write().reset();

                return Err(general_fault(
                    "auto schedule 'Waiting' with no start time!".to_string(),
                ));
            }
        }
        _ => unreachable!(),
    };

    if sleep_ms <= 0 {
        log::warn!("CHECK 1");

        return mister_auto_schedule_check(cfg.as_ref()).await;
    }

    match select(
        mode_changed_sub.next_message(),
        Timer::after(Duration::from_millis(sleep_ms as u64)),
    )
    .await
    {
        Either::First(r) => match r {
            WaitResult::Lagged(count) => {
                log::warn!(
                    "mister mode changed subscriber lagged by {} messages",
                    count
                );

                // Ignore
                Ok(())
            }
            WaitResult::Message(_) => {
                log::info!("Mister mode changed, resetting auto scheduler");
                ACTIVE_AUTO.write().reset();

                Ok(())
            }
        },
        Either::Second(_) => mister_auto_schedule_check(cfg.as_ref()).await,
    }
}

async fn mister_auto_schedule_start(cfg: &ConfigInstance, idx: usize) -> Result<()> {
    let (rh, run_secs) = get_auto_schedule(cfg)?;

    match ACTIVE_AUTO.write() {
        mut wr => {
            wr.reset();
            wr.idx = idx;
            wr.mode = AutoScheduleMode::Pending;
        }
    }

    log::info!(
        "Started mister auto schedule [rh: {}, run_secs: {}]",
        rh,
        run_secs
    );

    Ok(())
}

async fn mister_auto_schedule_next(cfg: &ConfigInstance) -> Result<()> {
    let cur_idx = ACTIVE_AUTO.read().idx;
    if cfg.mister_auto_rh_schedule.len() >= cur_idx + 2 {
        mister_auto_schedule_start(cfg, cur_idx + 1).await
    } else {
        mister_auto_schedule_start(cfg, 0).await
    }
}

async fn mister_auto_schedule_check(cfg: &ConfigInstance) -> Result<()> {
    let (target_rh, run_secs) = get_auto_schedule(cfg)?;

    match sensor::METRICS.read().clone() {
        Some(metrics) => match ACTIVE_AUTO.read().mode {
            AutoScheduleMode::Pending => {
                let rh_on = cfg.mister_auto_on_rh(target_rh);
                let rh_off = target_rh;

                if metrics.rh >= rh_on && metrics.rh <= rh_off {
                    match ACTIVE_AUTO.write() {
                        mut wr => {
                            wr.start_time = get_time_ms();
                            wr.mode = AutoScheduleMode::Running;
                        }
                    }
                }

                Ok(())
            }
            AutoScheduleMode::Running => {
                if ACTIVE_AUTO.read().running_ms() >= run_secs * 1000 {
                    mister_auto_schedule_next(cfg).await?;
                }

                Ok(())
            }
            _ => unreachable!(),
        },
        None => Err(general_fault(
            "failed to check auto schedule - no sensor metrics".to_string(),
        )),
    }
}

fn get_auto_schedule(cfg: &ConfigInstance) -> Result<(f32, u32)> {
    match ACTIVE_AUTO.read().get_auto_schedule(cfg) {
        Some((rh, run_secs)) => Ok((rh, run_secs)),
        None => {
            ACTIVE_AUTO.write().reset();

            Err(general_fault(format!(
                "no mister auto schedule found for idx: {}",
                ACTIVE_AUTO.read().idx
            )))
        }
    }
}

#[embassy_executor::task]
async fn mister_status_led_task(
    _cfg: Config,
    status_led_pin: GpioPin<Unknown, STATUS_LED_GPIO_PIN>,
    mut status_changed_sub: StatusChangedSubscriber,
) {
    let mut status_led_pin = status_led_pin.into_push_pull_output();

    loop {
        if let Err(e) =
            mister_status_led_task_poll(&mut status_led_pin, &mut status_changed_sub).await
        {
            log::warn!("mister status led task poll failed: {:?}", e);

            // Some sleep to avoid thrashing.
            Timer::after(Duration::from_millis(50)).await;
            continue;
        }
    }
}

async fn mister_status_led_task_poll(
    status_led_pin: &mut GpioPin<Output<PushPull>, STATUS_LED_GPIO_PIN>,
    status_changed_sub: &mut StatusChangedSubscriber,
) -> Result<()> {
    match select(
        status_changed_sub.next_message(),
        Timer::after(Duration::from_millis(400)),
    )
    .await
    {
        Either::First(r) => match r {
            WaitResult::Lagged(count) => {
                log::warn!("status change subscriber lagged by {} messages", count);

                // Ignore
                return Ok(());
            }
            WaitResult::Message(status) => match status {
                Status::Off => {
                    if status_led_pin.is_set_high().map_err(map_infallible_err)? {
                        status_led_pin.set_low().map_err(map_infallible_err)?;
                    }
                }
                Status::On => {
                    if status_led_pin.is_set_low().map_err(map_infallible_err)? {
                        status_led_pin.set_high().map_err(map_infallible_err)?;
                    }
                }
                Status::Fault => {
                    if status_led_pin.is_set_low().map_err(map_infallible_err)? {
                        status_led_pin.set_high().map_err(map_infallible_err)?;
                    }
                }
            },
        },
        Either::Second(_) => {
            // Blink (alternate)
            if matches!(STATUS.read().as_ref(), Some(&Status::Fault)) {
                if status_led_pin.is_set_low().map_err(map_infallible_err)? {
                    status_led_pin.set_high().map_err(map_infallible_err)?;
                } else {
                    status_led_pin.set_low().map_err(map_infallible_err)?;
                }
            }
        }
    }

    Ok(())
}

async fn change_status_from_mode(
    mode: Mode,
    mister_pwr_pin: &mut GpioPin<Output<PushPull>, MISTER_POWER_GPIO_PIN>,
    status_changed_pub: &mut StatusChangedPublisher,
) -> Result<()> {
    match mode {
        Mode::On => change_status(Status::On, mister_pwr_pin, status_changed_pub).await?,
        Mode::Off => change_status(Status::Off, mister_pwr_pin, status_changed_pub).await?,
        // Start 'Off' for Auto.
        Mode::Auto => change_status(Status::Off, mister_pwr_pin, status_changed_pub).await?,
    }

    Ok(())
}

async fn change_status(
    status: Status,
    mister_pwr_pin: &mut GpioPin<Output<PushPull>, MISTER_POWER_GPIO_PIN>,
    status_changed_pub: &mut StatusChangedPublisher,
) -> Result<()> {
    match status {
        Status::Off => {
            if mister_pwr_pin.is_set_high().map_err(map_infallible_err)? {
                mister_pwr_pin.set_low().map_err(map_infallible_err)?;
            }
        }
        Status::On => {
            if mister_pwr_pin.is_set_low().map_err(map_infallible_err)? {
                mister_pwr_pin.set_high().map_err(map_infallible_err)?;
            }
        }
        Status::Fault => {
            if mister_pwr_pin.is_set_high().map_err(map_infallible_err)? {
                mister_pwr_pin.set_low().map_err(map_infallible_err)?;
            }
        }
    }

    if match STATUS.read().as_ref() {
        None => true,
        Some(v) => !v.eq(&status),
    } {
        log::info!("Mister status changed to: {:?}", status);

        let _ = STATUS.write().insert(status);
        status_changed_pub.publish_immediate(status);
    }

    Ok(())
}

async fn toggle_mode(
    storage: &mut FlashStorage,
    mode_changed_pub: &mut ModeChangedPublisher,
) -> Result<Mode> {
    let next_mode = match ACTIVE_MODE.read().clone() {
        None => Mode::Auto,
        Some(mode) => {
            let mode_u8 = mode as u8;
            if mode_u8 + 1 <= Mode::max() {
                Mode::from(mode_u8 + 1)
            } else {
                Mode::Auto
            }
        }
    };

    store_mode(storage, next_mode, mode_changed_pub).await?;

    Ok(next_mode)
}

async fn load_mode(storage: &mut FlashStorage, mode_changed_pub: &mut ModeChangedPublisher) {
    let mut bytes = [0u8; 1];
    let mode = match storage.read(MODE_FLASH_ADDR, &mut bytes) {
        Ok(_) => {
            let mode_u8 = u8::from_be_bytes(bytes);
            if mode_u8 >= Mode::min() && mode_u8 <= Mode::max() {
                let mode = Mode::from(u8::from_be_bytes(bytes));
                log::info!("Restored previous mode '{}' from flash", mode);
                mode
            } else {
                Mode::Auto
            }
        }
        Err(_) => Mode::Auto,
    };

    let _ = ACTIVE_MODE.write().insert(mode);
    mode_changed_pub.publish_immediate(mode);
}

async fn store_mode(
    storage: &mut FlashStorage,
    mode: Mode,
    mode_changed_pub: &mut ModeChangedPublisher,
) -> Result<()> {
    let mode_u8 = mode as u8;
    storage
        .write(MODE_FLASH_ADDR, mode_u8.to_be_bytes().as_ref())
        .map_err(|e| {
            general_fault(format!(
                "Failed to persist active mode to flash storage: {:?}",
                e
            ))
        })?;

    log::info!("Persisted mode '{}' to flash", mode);

    let _ = ACTIVE_MODE.write().insert(mode);
    mode_changed_pub.publish_immediate(mode);

    Ok(())
}

pub(crate) fn is_mode_auto() -> bool {
    matches!(ACTIVE_MODE.read().as_ref(), Some(&Mode::Auto))
}

// Models

#[derive(Copy, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub(crate) enum Mode {
    Auto = 1,
    Off = 2,
    On = 3,
}

impl Mode {
    pub(crate) fn min() -> u8 {
        1
    }
    pub(crate) fn max() -> u8 {
        3
    }
}

impl Display for Mode {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            Mode::Auto => write!(f, "Auto"),
            Mode::Off => write!(f, "Off"),
            Mode::On => write!(f, "On"),
        }
    }
}

impl From<u8> for Mode {
    fn from(value: u8) -> Self {
        if value == 1 {
            Self::Auto
        } else if value == 2 {
            Self::Off
        } else if value == 3 {
            Self::On
        } else {
            Self::Auto
        }
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

#[derive(Copy, Clone, PartialEq, Debug, Serialize)]
pub(crate) enum Status {
    Off,
    On,
    Fault,
}
