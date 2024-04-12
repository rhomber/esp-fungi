use esp_wifi::wifi::log_timestamp;

pub fn get_time_ms() -> u32 {
    unsafe { log_timestamp() }
}
