pub mod adb;
pub mod system;

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;
