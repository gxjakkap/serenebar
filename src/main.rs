// debug builds keep a console so panics show up. Release builds have none.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// elsewhere only the platform-free modules build, for their tests
#![cfg_attr(not(windows), allow(dead_code))]
#[cfg(windows)]
mod app;
mod apps;
mod calendar;
#[cfg(windows)]
mod controls;
mod dates;
mod dock;
#[cfg(windows)]
mod feeds;
#[cfg(windows)]
mod gfx;
mod glass;
mod google;
mod http;
mod limits;
#[cfg(windows)]
mod panel;
#[cfg(windows)]
mod prefs;
mod settings;
#[cfg(windows)]
mod system;
#[cfg(windows)]
mod taskbar;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod ui;
mod weather;

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    app::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("serenebar runs on Windows 10/11");
}
