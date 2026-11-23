pub mod adb;
pub mod antigravity;
pub mod media;
pub mod network;
pub mod system;

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;
