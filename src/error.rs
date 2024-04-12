use alloc::string::String;
use core::convert::Infallible;
use core::fmt;
use display_interface::DisplayError;
use embassy_executor::SpawnError;
use embassy_sync::pubsub::Error as EmbassyPubSubError;
use esp_wifi::wifi::WifiError;
use esp_wifi::InitializationError;

pub type Result<T> = core::result::Result<T, Error>;

pub enum Error {
    #[allow(dead_code)]
    GeneralFault {
        msg: String,
    },
    Infallible,
    WifiInit {
        e: InitializationError,
    },
    Wifi {
        e: WifiError,
    },
    EmbassySpawn {
        e: SpawnError,
    },
    EmbassyPubSub {
        e: EmbassyPubSubError,
    },
    Display {
        e: DisplayError,
    },
    DisplayDraw {
        msg: String,
    },
    SensorFault {
        msg: String,
    },
    SerdeJson {
        e: serde_json::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::GeneralFault { msg } => {
                write!(f, "A general fault occurred: {}", msg)
            }
            Error::Infallible => {
                write!(f, "Unexpected infallible error encountered")
            }
            Error::WifiInit { e } => {
                write!(f, "Failed to init WIFI: {:?}", e)
            }
            Error::Wifi { e } => {
                write!(f, "WIFI error: {:?}", e)
            }
            Error::EmbassySpawn { e } => {
                write!(f, "Embassy spawn error: {:?}", e)
            }
            Error::EmbassyPubSub { e } => {
                write!(f, "Embassy pub/sub error: {:?}", e)
            }
            Error::Display { e } => {
                write!(f, "Display error: {:?}", e)
            }
            Error::DisplayDraw { msg } => {
                write!(f, "Display draw error: {:?}", msg)
            }
            Error::SensorFault { msg } => {
                write!(f, "Sensor fault: {:?}", msg)
            }
            Error::SerdeJson { e } => {
                write!(f, "JSON error: {:?}", e)
            }
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[allow(dead_code)]
pub(crate) fn general_fault(msg: String) -> Error {
    Error::GeneralFault { msg }
}

#[allow(dead_code)]
pub(crate) fn sensor_fault(msg: String) -> Error {
    Error::SensorFault { msg }
}

pub(crate) fn map_wifi_init_err(e: InitializationError) -> Error {
    Error::WifiInit { e }
}

pub(crate) fn map_wifi_err(e: WifiError) -> Error {
    Error::Wifi { e }
}

pub(crate) fn map_embassy_spawn_err(e: SpawnError) -> Error {
    Error::EmbassySpawn { e }
}

pub(crate) fn map_embassy_pub_sub_err(e: EmbassyPubSubError) -> Error {
    Error::EmbassyPubSub { e }
}

pub(crate) fn map_display_err(e: DisplayError) -> Error {
    Error::Display { e }
}

pub(crate) fn display_draw_err(msg: String) -> Error {
    Error::DisplayDraw { msg }
}

pub(crate) fn map_infallible_err(_: Infallible) -> Error {
    Error::Infallible
}

pub(crate) fn map_json_err(e: serde_json::Error) -> Error {
    Error::SerdeJson { e }
}
