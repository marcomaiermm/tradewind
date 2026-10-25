#[macro_use]
pub mod i18n;
mod adapters;
pub mod application;
pub mod core;
pub mod ui;

pub const APP_VERSION: &str = env!("TRADEWIND_VERSION");
