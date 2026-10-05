//! Small configuration and ownership adapter for unmodified upstream usvfs.
mod child_output;
#[cfg(windows)]
mod shortcut;
#[cfg(windows)]
pub use shortcut::persist_shortcut;
#[cfg(windows)]
mod hidden_windows;
#[cfg(windows)]
pub use hidden_windows::detach_console;
#[cfg(windows)]
pub use hidden_windows::show_error;
#[cfg(any(windows, test))]
mod configuration;
mod error;
pub use child_output::CapturedOutput;
pub use child_output::CapturedStream;
pub use child_output::ExecutionCapture;
#[cfg(windows)]
pub use child_output::PrivateStreams;
#[cfg(windows)]
mod process;
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
pub use launch_inputs::ResolvedLaunch;
#[cfg(windows)]
pub use managed::ManagedProcess;
#[cfg(windows)]
pub use managed::SupervisedExit;
#[cfg(windows)]
pub use managed::supervise;
pub use profile::ActivationSource;
pub use profile::EffectivePlugin;
#[cfg(any(windows, test))]
pub use profile::ProfileMappingInput;
#[cfg(any(windows, test))]
pub use profile::ProfileMappings;
pub use profile::ProfileProjectionError;
pub use profile::ProfileProjectionInput;
pub use profile::ProfileText;
pub use profile::ProjectedProfile;
pub use profile::VisibleProfileFile;
pub use profile::build_profile_projection;
#[cfg(any(windows, test))]
pub use profile::profile_mappings;
#[cfg(any(windows, test))]
mod launch_inputs;
