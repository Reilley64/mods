//! Small configuration and ownership adapter for unmodified upstream usvfs.
#[cfg(any(windows, test))]
mod configuration;
mod error;
#[cfg(windows)]
mod process;
#[cfg(windows)]
mod usvfs;

#[cfg(any(windows, test))]
pub use configuration::PathMapping;
#[cfg(any(windows, test))]
pub use configuration::ProviderRoot;
#[cfg(any(windows, test))]
pub use configuration::ViewConfiguration;
pub use error::ExecutionError;
pub use error::NativeFailure;
#[cfg(windows)]
pub use process::HookedProcess;
#[cfg(windows)]
pub use process::LaunchRequest;
#[cfg(windows)]
pub use usvfs::VirtualGameView;
