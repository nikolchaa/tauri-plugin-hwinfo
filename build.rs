const COMMANDS: &[&str] = &[
    "get_system_info",
    "get_cpu_info",
    "get_gpu_info",
    "get_memory_info",
    "get_storage_info",
    "get_network_info",
    "get_display_info",
    "get_battery_info",
    "get_board_info",
    "get_os_info",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
