use alloc::format;
use alloc::string::String;
use core::fmt;
use esp_wifi::wifi::WifiError;
use esp_wifi::InitializationError;

pub type Result<T> = core::result::Result<T, Error>;

pub enum Error {
    GeneralFault { msg: String },
    WifiInit { e: InitializationError },
    Wifi { e: WifiError },
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
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

pub(crate) fn general_fault(msg: String) -> Error {
    Error::GeneralFault { msg }
}

pub(crate) fn map_wifi_init_err(e: InitializationError) -> Error {
    Error::WifiInit { e }
}

pub(crate) fn map_wifi_err(e: WifiError) -> Error {
    Error::Wifi { e }
}
