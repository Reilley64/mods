//! Small configuration and ownership adapter for unmodified upstream usvfs.
#[cfg(any(windows, test))]
mod configuration;
mod error;
#[cfg(windows)]
mod process;
#[cfg(any(windows, test))]
mod profile;
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

#[cfg(any(windows, test))]
mod managed;

#[cfg(windows)]
pub use launch_inputs::CallerSnapshot;
#[cfg(windows)]
pub use launch_inputs::InheritedStreams;
#[cfg(any(windows, test))]
pub use launch_inputs::LaunchInputError;
#[cfg(windows)]
pub use managed::ManagedProcess;
#[cfg(windows)]
pub use managed::SupervisedExit;
#[cfg(windows)]
pub use managed::supervise;
#[cfg(any(windows, test))]
pub use profile::ActivationSource;
#[cfg(any(windows, test))]
pub use profile::EffectivePlugin;
#[cfg(any(windows, test))]
pub use profile::ProfileConfiguration;
#[cfg(any(windows, test))]
pub use profile::ProfileConfigurationError;
#[cfg(any(windows, test))]
pub use profile::ProfileConfigurationInput;
#[cfg(any(windows, test))]
pub use profile::ProfileText;
#[cfg(any(windows, test))]
pub use profile::ProfileWarning;
#[cfg(any(windows, test))]
pub use profile::VisibleProfileFile;
#[cfg(any(windows, test))]
pub use profile::build_profile_configuration;
#[cfg(any(windows, test))]
mod launch_inputs;
