#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub use windows::{clipboard, focus, input, placement, uia};
