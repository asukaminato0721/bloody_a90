//! Saved areas of the A90 flash, in 16-bit words. Shared by the hidraw
//! driver and the browser version.

pub const SENSITIVITY: usize = 0x2400;
pub const BUTTONS: usize = 0x2600;
/// Firmware 07 keeps command handlers in page 3d00. Writes with mode 80 erase
/// a whole 256-word page at a page-aligned address, so saves stay below it.
pub const BUTTONS_END: usize = 0x3d00;
pub const WORDS: usize = 0x4000;
