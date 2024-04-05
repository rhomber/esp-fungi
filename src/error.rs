use alloc::string::String;
use core::fmt;
use embassy_executor::SpawnError;
use esp_wifi::wifi::WifiError;
use esp_wifi::InitializationError;

pub type Result<T> = core::result::Result<T, Error>;

pub enum Error {
    #[allow(dead_code)]
    GeneralFault {
        msg: String,
    },
    WifiInit {
        e: InitializationError,
    },
    Wifi {
        e: WifiError,
    },
    EmbassySpawn {
        e: SpawnError,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::GeneralFault { msg } => {
                write!(f, "A general fault occurred: {}", msg)
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

pub(crate) fn map_wifi_init_err(e: InitializationError) -> Error {
    Error::WifiInit { e }
}

pub(crate) fn map_wifi_err(e: WifiError) -> Error {
    Error::Wifi { e }
}

pub(crate) fn map_embassy_spawn_err(e: SpawnError) -> Error {
    Error::EmbassySpawn { e }
}
