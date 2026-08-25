//! Scan orchestration.
//!
//! Every collector is infallible from the caller's point of view: a probe that
//! fails records a warning in [`Ctx`] and leaves the corresponding fields
//! `None`, so a partial answer is always better than an error.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::models::*;

pub(crate) mod battery;
pub(crate) mod board;
pub(crate) mod compute;
pub(crate) mod cpu;
pub(crate) mod display;
pub(crate) mod gpu;
pub(crate) mod memory;
pub(crate) mod network;
pub(crate) mod os;
pub(crate) mod storage;
#[cfg(feature = "vulkan")]
pub(crate) mod vulkan;

/// Shared state for a single scan.
pub struct Ctx {
    pub mode: ScanMode,
    pub detail: DetailLevel,
    warnings: Vec<String>,
    /// Monitors handed in by the Tauri runtime, since the collectors run off
    /// the main thread and cannot reach the window system themselves on every
    /// platform.
    pub(crate) monitors: Vec<MonitorHint>,
    /// Output of helper processes, memoised for the life of one scan.
    probes: std::collections::HashMap<String, Result<String, String>>,
}

/// The portable slice of monitor data Tauri can give us.
#[derive(Debug, Clone)]
pub struct MonitorHint {
    pub name: Option<String>,
    pub width: u32,
    pub height: u32,
    pub position_x: i32,
    pub position_y: i32,
    pub scale_factor: f64,
    pub is_primary: bool,
}

impl Ctx {
    pub fn new(mode: ScanMode, detail: DetailLevel) -> Self {
        Self {
            mode,
            detail,
            warnings: Vec::new(),
            monitors: Vec::new(),
            probes: std::collections::HashMap::new(),
        }
    }

    /// Whether the scan is allowed to do work at `level` or deeper.
    pub fn wants(&self, level: DetailLevel) -> bool {
        self.detail >= level
    }

    /// Run `probe` at most once per scan, keyed by `key`.
    ///
    /// Several sections want the same expensive answer - `system_profiler` on
    /// macOS takes the better part of a second per data type, and the display
    /// inventory is needed by both the GPU and display collectors.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn cached<F>(&mut self, key: &str, probe: F) -> Result<String, String>
    where
        F: FnOnce() -> Result<String, String>,
    {
        if let Some(hit) = self.probes.get(key) {
            return hit.clone();
        }
        let result = probe();
        self.probes.insert(key.to_string(), result.clone());
        result
    }

    pub fn with_monitors(mut self, monitors: Vec<MonitorHint>) -> Self {
        self.monitors = monitors;
        self
    }

    /// Record a non-fatal problem. Duplicates are collapsed.
    pub fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if !self.warnings.contains(&msg) {
            self.warnings.push(msg);
        }
    }

    pub fn into_warnings(self) -> Vec<String> {
        self.warnings
    }

    /// `value` in unsafe mode, `None` otherwise.
    pub fn redact<T>(&self, value: Option<T>) -> Option<T> {
        self.mode.redact(value)
    }
}

/// Collect the requested sections.
pub fn run(mut ctx: Ctx, sections: &[Section]) -> SystemInfo {
    let started = Instant::now();
    let want = |s: Section| sections.contains(&s);

    // Run the Vulkan probe once up front so a machine with several adapters
    // does not pay for repeated loader initialisation. It is a device probe,
    // not a free read - on a hybrid laptop it can wake a sleeping dGPU - so it
    // waits for the capabilities tier.
    #[cfg(feature = "vulkan")]
    let vulkan = if want(Section::Gpu) && ctx.wants(DetailLevel::Capabilities) {
        vulkan::probe(&mut ctx)
    } else {
        Vec::new()
    };
    #[cfg(not(feature = "vulkan"))]
    let vulkan: Vec<()> = Vec::new();

    let cpu = want(Section::Cpu).then(|| cpu::collect(&mut ctx));
    let gpu = want(Section::Gpu).then(|| gpu::collect(&mut ctx, &vulkan));
    let memory = want(Section::Memory).then(|| memory::collect(&mut ctx));
    let storage = want(Section::Storage).then(|| storage::collect(&mut ctx));
    let network = want(Section::Network).then(|| network::collect(&mut ctx));
    let display = want(Section::Display).then(|| display::collect(&mut ctx));
    let battery = want(Section::Battery).then(|| battery::collect(&mut ctx));
    let board = want(Section::Board).then(|| board::collect(&mut ctx));
    let os = want(Section::Os).then(|| os::collect(&mut ctx));

    let duration_ms = started.elapsed().as_millis() as u64;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    SystemInfo {
        scan: ScanMeta {
            version: 1,
            mode: ctx.mode,
            detail: ctx.detail,
            sections: sections.to_vec(),
            duration_ms,
            timestamp,
            warnings: ctx.into_warnings(),
        },
        cpu,
        gpu,
        memory,
        storage,
        network,
        display,
        battery,
        board,
        os,
    }
}

/// Bytes to whole mebibytes.
pub(crate) fn to_mb(bytes: u64) -> u64 {
    bytes / 1024 / 1024
}

/// Trim, then drop values that carry no information. Firmware tables are full
/// of `"To Be Filled By O.E.M."`, `"Default string"` and rows of `0`.
pub(crate) fn clean(value: impl AsRef<str>) -> Option<String> {
    let v = value.as_ref().trim().trim_matches('\0').trim();
    if v.is_empty() {
        return None;
    }
    const PLACEHOLDERS: [&str; 26] = [
        "to be filled by o.e.m.",
        "to be filled by oem",
        "default string",
        "base board",
        "chassis",
        "no asset tag",
        "no asset information",
        "asset tag",
        // SMBIOS asset-tag defaults shipped by most OEMs.
        "tag 12345",
        "asset-1234567890",
        "0123456789",
        "123456789",
        "system manufacturer",
        "system product name",
        "system version",
        "system serial number",
        "chassis manufacture",
        "not specified",
        "not applicable",
        "not available",
        "no enclosure",
        "unknown",
        "none",
        "n/a",
        // Windows uses this stand-in whenever a drive has no vendor string.
        "(standard disk drives)",
        "standard disk drives",
    ];
    let lowered = v.to_ascii_lowercase();
    if PLACEHOLDERS.contains(&lowered.as_str()) {
        return None;
    }
    // Firmware pads unset fields with a repeated filler character: "0",
    // "000000", "....". A run of zeroes is never a real value at any length;
    // other punctuation runs need at least two characters to be recognisable
    // as padding rather than content.
    let first = v.as_bytes()[0];
    let all_same = v.bytes().all(|b| b == first);
    if all_same && (first == b'0' || (!first.is_ascii_alphanumeric() && v.len() > 1)) {
        return None;
    }

    Some(v.to_string())
}

/// `clean`, applied to an optional value.
pub(crate) fn clean_opt(value: Option<impl AsRef<str>>) -> Option<String> {
    value.and_then(clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_keeps_real_values() {
        assert_eq!(clean("AMD Ryzen 9 5900X"), Some("AMD Ryzen 9 5900X".into()));
        assert_eq!(
            clean("  X570 AORUS ELITE \0"),
            Some("X570 AORUS ELITE".into())
        );
        // Leading zeroes are fine as long as the value is not *all* zeroes.
        assert_eq!(clean("0x00004119"), Some("0x00004119".into()));
        assert_eq!(clean("0001"), Some("0001".into()));
    }

    #[test]
    fn clean_drops_firmware_padding() {
        assert_eq!(clean(""), None);
        assert_eq!(clean("   "), None);
        assert_eq!(clean("0"), None);
        assert_eq!(clean("00000000"), None);
        assert_eq!(clean("...."), None);
        assert_eq!(clean("To Be Filled By O.E.M."), None);
        assert_eq!(clean("Default string"), None);
        assert_eq!(clean("Not Applicable"), None);
        assert_eq!(clean("(Standard disk drives)"), None);
        assert_eq!(clean("No Asset Tag"), None);
    }

    #[test]
    fn safe_mode_redacts_identifiers() {
        assert_eq!(ScanMode::Safe.redact(Some("serial")), None);
        assert_eq!(ScanMode::Unsafe.redact(Some("serial")), Some("serial"));
    }
}
