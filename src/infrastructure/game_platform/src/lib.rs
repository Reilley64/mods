#![cfg_attr(test, feature(fn_traits))]

mod adapter;
mod bound_game;
mod file_version;
mod fs_access;
mod known_folders;
mod ports;
mod profile_sources;
mod registry;
mod resolution;
mod separation;
mod steam;
mod version;

pub use adapter::GamePlatformAdapter;
