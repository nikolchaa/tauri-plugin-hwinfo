//! Print a hardware scan as JSON.
//!
//! ```sh
//! cargo run --example dump                       # safe / summary
//! cargo run --example dump -- capabilities       # add GPU API probing
//! cargo run --example dump -- full unsafe        # everything, identifiers included
//! ```

use tauri_plugin_hwinfo::{DetailLevel, ScanMode, ScanOptions};

fn main() {
    let mut options = ScanOptions::default();

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "safe" => options.mode = ScanMode::Safe,
            "unsafe" => options.mode = ScanMode::Unsafe,
            "summary" => options.detail = DetailLevel::Summary,
            "capabilities" => options.detail = DetailLevel::Capabilities,
            "full" => options.detail = DetailLevel::Full,
            other => {
                eprintln!("unknown argument `{other}`");
                eprintln!("usage: dump [safe|unsafe] [summary|capabilities|full]");
                std::process::exit(2);
            }
        }
    }

    let info = tauri_plugin_hwinfo::scan_blocking(options);
    println!("{}", serde_json::to_string_pretty(&info).unwrap());
}
