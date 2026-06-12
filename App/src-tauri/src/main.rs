// Windows 隐藏控制台
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ecoalert_lib::run()
}
