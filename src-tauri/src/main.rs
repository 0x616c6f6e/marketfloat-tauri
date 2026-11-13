#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod desktop;
mod market;
mod market_state;
mod settings;

fn main() {
    desktop::run();
}
