#![feature(fn_traits)]
#![forbid(unsafe_code)]

pub mod conflicts;
pub mod environment;
mod errors;
pub mod execution;
pub mod installation;
pub mod ports;
pub mod settings;

pub use errors::ErrorCode;
pub use errors::ErrorMarker;
