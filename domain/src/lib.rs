#![forbid(unsafe_code)]

mod environment;
mod game;

pub use environment::EnvironmentName;
pub use environment::EnvironmentRoot;
pub use environment::EnvironmentSchemaVersion;
pub use environment::InvalidEnvironmentName;
pub use environment::InvalidEnvironmentRoot;
pub use environment::UnsupportedEnvironmentSchemaVersion;
pub use game::GameBinding;
pub use game::GameInstallationPath;
pub use game::InvalidGameInstallationPath;
pub use game::InvalidSteamBuildId;
pub use game::SteamAppId;
pub use game::SteamBuildId;
pub use game::UnsupportedSteamAppId;
