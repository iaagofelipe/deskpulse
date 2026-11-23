pub mod adb;
pub mod ai;
pub mod antigravity;
pub mod github;
pub mod media;
pub mod network;
pub mod system;
pub mod weather;

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;
