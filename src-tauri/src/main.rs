// Prevents an extra console window on Windows in release builds; harmless elsewhere.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    focus_companion_lib::run();
}
