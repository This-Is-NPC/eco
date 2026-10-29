//! The Start menu entry of eco: `eco start` as a program with no console
//! window, which tells a failure as a notification (docs/design.md §15).
#![cfg_attr(windows, windows_subsystem = "windows")]

#[tokio::main]
async fn main() -> std::process::ExitCode {
    eco::open_window().await
}
