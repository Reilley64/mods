#![feature(fn_traits)]

mod adapter;
mod entry;
mod error;
mod extract;
mod fomod;
mod index;
mod limits;
mod path;
mod rar;
mod seven_zip;
mod source;
mod zip;

pub use adapter::ArchiveAdapter;
