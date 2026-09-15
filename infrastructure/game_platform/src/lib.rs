mod adapter;
mod cancellation;
mod fs_access;
mod known_folders;
mod ports;
mod profile_sources;
mod registry;
mod resolution;
mod separation;
mod steam;

pub use adapter::GamePlatformAdapter;

#[cfg(test)]
mod tests;
