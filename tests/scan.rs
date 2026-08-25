//! End-to-end checks against whatever hardware the test is running on.
//!
//! These assert on the shape of the contract rather than on specific values,
//! so they hold on any machine.

use tauri_plugin_hwinfo::{DetailLevel, ScanMode, ScanOptions, Section, SystemInfo};

fn scan(mode: ScanMode, sections: Option<Vec<Section>>) -> SystemInfo {
    tauri_plugin_hwinfo::scan_blocking(ScanOptions {
        mode,
        detail: DetailLevel::Full,
        sections,
    })
}

fn scan_at(detail: DetailLevel, sections: &[Section]) -> SystemInfo {
    tauri_plugin_hwinfo::scan_blocking(ScanOptions {
        mode: ScanMode::Safe,
        detail,
        sections: Some(sections.to_vec()),
    })
}

#[test]
fn section_filter_collects_only_what_was_asked_for() {
    let info = scan(ScanMode::Safe, Some(vec![Section::Os, Section::Memory]));

    assert_eq!(info.scan.sections, vec![Section::Os, Section::Memory]);
    assert!(info.os.is_some());
    assert!(info.memory.is_some());

    assert!(info.cpu.is_none());
    assert!(info.gpu.is_none());
    assert!(info.storage.is_none());
    assert!(info.network.is_none());
    assert!(info.battery.is_none());
    assert!(info.board.is_none());
}

#[test]
fn an_empty_section_list_means_everything() {
    let info = scan(ScanMode::Safe, Some(Vec::new()));
    assert_eq!(info.scan.sections.len(), Section::ALL.len());
}

#[test]
fn safe_mode_withholds_every_identifier() {
    let info = scan(ScanMode::Safe, None);

    for cpu in info.cpu.iter().flatten() {
        assert!(cpu.serial.is_none(), "cpu serial leaked in safe mode");
    }
    for gpu in info.gpu.iter().flatten() {
        assert!(gpu.uuid.is_none(), "gpu uuid leaked in safe mode");
    }
    for module in info.memory.iter().flat_map(|m| &m.modules) {
        assert!(module.serial.is_none(), "memory serial leaked in safe mode");
    }
    for disk in info.storage.iter().flat_map(|s| &s.disks) {
        assert!(disk.serial.is_none(), "disk serial leaked in safe mode");
    }
    for interface in info.network.iter().flatten() {
        assert!(interface.mac_address.is_none(), "MAC leaked in safe mode");
        assert!(interface.ip_networks.is_none(), "IPs leaked in safe mode");
    }
    for battery in info.battery.iter().flatten() {
        assert!(
            battery.serial.is_none(),
            "battery serial leaked in safe mode"
        );
    }

    let board = info.board.expect("board section requested");
    assert!(board.serial.is_none());
    assert!(board.chassis.serial.is_none());
    assert!(board.system.uuid.is_none());
    assert!(board.system.serial.is_none());

    let os = info.os.expect("os section requested");
    assert!(os.hostname.is_none());
    assert!(os.machine_id.is_none());
    assert!(os.user.is_none());
}

#[test]
fn core_facts_are_always_populated() {
    let info = scan(ScanMode::Safe, None);

    let cpus = info.cpu.expect("cpu section requested");
    assert!(!cpus.is_empty(), "no CPU packages found");
    for cpu in &cpus {
        assert!(cpu.threads > 0, "a package reported zero threads");
        assert!(!cpu.architecture.is_empty());
        // The merge folds the observed clock into the maximum, so this holds
        // even on VMs that advertise no rated ceiling of their own.
        if let Some(current) = cpu.current_frequency {
            assert!(
                cpu.max_frequency >= current,
                "max clock below current clock"
            );
        }
    }

    let memory = info.memory.expect("memory section requested");
    assert!(memory.total_mb > 0, "no physical memory reported");
    assert!(memory.used_mb <= memory.total_mb);

    let os = info.os.expect("os section requested");
    assert!(!os.name.is_empty());
    assert!(!os.family.is_empty());
    assert!(os.boot_time_secs > 0);
}

#[test]
fn scan_metadata_describes_the_run() {
    let info = scan(ScanMode::Safe, Some(vec![Section::Os]));

    assert_eq!(info.scan.version, 1);
    assert_eq!(info.scan.mode, ScanMode::Safe);
    assert_eq!(info.scan.detail, DetailLevel::Full);
    assert!(info.scan.timestamp > 0);
}

#[test]
fn summary_omits_itemised_detail() {
    let info = scan_at(
        DetailLevel::Summary,
        &[Section::Cpu, Section::Memory, Section::Storage],
    );

    for cpu in info.cpu.iter().flatten() {
        assert!(cpu.cores.is_empty(), "per-core list present at summary");
        assert!(cpu.usage_percent.is_none(), "load sampled at summary");
        // The package facts a caller actually asked for are still there.
        assert!(cpu.threads > 0);
        assert!(!cpu.model.is_empty());
    }

    let memory = info.memory.expect("memory requested");
    assert!(memory.modules.is_empty(), "DIMMs enumerated at summary");
    assert!(memory.total_mb > 0, "totals should survive");

    let storage = info.storage.expect("storage requested");
    assert!(storage.disks.is_empty(), "physical disks probed at summary");
    assert!(!storage.volumes.is_empty(), "volumes should survive");
}

#[test]
fn full_restores_itemised_detail() {
    let info = scan_at(DetailLevel::Full, &[Section::Cpu, Section::Storage]);

    for cpu in info.cpu.iter().flatten() {
        assert!(!cpu.cores.is_empty(), "per-core list missing at full");
        assert!(cpu.usage_percent.is_some(), "load not sampled at full");
    }

    let storage = info.storage.expect("storage requested");
    assert!(!storage.disks.is_empty(), "no physical disks at full");
}

/// Adapters may legitimately be absent — a headless Linux CI runner has no
/// DRM nodes and no Vulkan loader — but anything reported must be well formed.
#[test]
fn gpu_entries_are_well_formed_at_capabilities() {
    let info = scan_at(DetailLevel::Capabilities, &[Section::Gpu]);

    for gpu in info.gpu.iter().flatten() {
        assert!(!gpu.manufacturer.is_empty(), "adapter with no manufacturer");
        assert!(!gpu.model.is_empty(), "adapter with no model");

        if let Some(hex) = &gpu.vendor_id_hex {
            // PCI vendor IDs are four hex digits, but software renderers
            // (Mesa's llvmpipe) report synthetic IDs beyond 16 bits.
            assert!(
                hex.starts_with("0x"),
                "vendor id hex `{hex}` lacks 0x prefix"
            );
            assert_eq!(
                u32::from_str_radix(&hex[2..], 16).ok(),
                gpu.vendor_id,
                "vendor id integer and hex disagree"
            );
        }
        if let Some(hex) = &gpu.device_id_hex {
            assert_eq!(
                u32::from_str_radix(&hex[2..], 16).ok(),
                gpu.device_id,
                "device id integer and hex disagree"
            );
        }
        if let Some(bus) = &gpu.pci_bus {
            // Windows only fills this through VK_EXT_pci_bus_info, so it can
            // be null — but when present it is a PCI address on every OS.
            let parts: Vec<&str> = bus.split([':', '.']).collect();
            assert_eq!(parts.len(), 4, "pci address `{bus}` is malformed");
            assert!(u16::from_str_radix(parts[0], 16).is_ok());
            assert!(u8::from_str_radix(parts[1], 16).is_ok());
        }
        if let Some(vram) = gpu.vram_mb {
            assert!(vram > 0, "adapter reported zero VRAM instead of null");
        }
        // Metal follows from the target rather than from a probe: every Mac
        // new enough to run this code has it - Intel included, since Catalina
        // made Metal-capable GPUs a requirement - and nothing else ever does.
        assert_eq!(
            gpu.api.metal,
            cfg!(target_os = "macos"),
            "metal flag disagrees with the platform"
        );
    }
}

/// Displays are absent on headless runners; present ones must be coherent.
///
/// Outside a Tauri runtime there are no monitor hints, so a platform that
/// cannot measure the *current* mode (Linux sysfs) reports a zero mode and
/// carries identity plus the panel's native mode instead.
#[test]
fn display_entries_are_well_formed() {
    let info = scan_at(DetailLevel::Full, &[Section::Display]);

    for display in info.display.iter().flatten() {
        // A virtualised display can report sentinel EDID identifiers all
        // round; a live mode is identification enough.
        let identified = !display.name.as_deref().unwrap_or_default().is_empty()
            || display.model.is_some()
            || display.manufacturer.is_some()
            || display.native_resolution.is_some()
            || display.resolution.width > 0;
        assert!(identified, "display entry carries no identifying fields");

        let mode = display.resolution;
        if mode.width > 0 && mode.height > 0 {
            if let Some(native) = display.native_resolution {
                // A scaled mode's native panel is at least as many pixels.
                assert!(
                    native.width * native.height >= mode.width * mode.height,
                    "native mode smaller than the current mode"
                );
            }
            if let Some(rate) = mode.refresh_rate_hz {
                assert!(
                    (24.0..=500.0).contains(&rate),
                    "implausible refresh rate {rate}"
                );
            }
        }
    }
}

/// The board section is where every platform must agree on identity basics.
#[test]
fn board_section_has_core_identity() {
    let info = scan_at(DetailLevel::Full, &[Section::Board]);

    let board = info.board.expect("board section requested");
    assert!(
        board.manufacturer.as_deref().is_some_and(|m| !m.is_empty()),
        "no board manufacturer reported"
    );
    assert!(board.bios.mode.as_deref().is_some_and(|m| !m.is_empty()));

    // Real hardware and CI virtual machines alike report a system product
    // name — the VMs' hypervisor stand-in ("VMware Virtual Platform") counts.
    assert!(
        board.system.product.is_some() || board.product.is_some(),
        "neither system nor board product reported"
    );
}

#[test]
fn summary_is_substantially_cheaper_than_full() {
    // Deliberately loose: this asserts the gating is wired up at all, not a
    // performance figure that would be flaky on shared CI hardware.
    let sections = [Section::Cpu, Section::Memory, Section::Storage];
    let summary = scan_at(DetailLevel::Summary, &sections);
    let full = scan_at(DetailLevel::Full, &sections);

    assert!(
        summary.scan.duration_ms < full.scan.duration_ms,
        "summary took {} ms, full took {} ms",
        summary.scan.duration_ms,
        full.scan.duration_ms
    );
}

// ---------------------------------------------------------------------------
// Shape contract: the same keys must serialize on every platform, with nulls
// standing in for platform gaps. These tests fail if anyone adds a
// `skip_serializing_if`, omits a field on one backend's path, or renames a
// field without updating the contract.
// ---------------------------------------------------------------------------

const DISK_KEYS: [&str; 11] = [
    "device",
    "model",
    "vendor",
    "kind",
    "bus",
    "sizeMb",
    "firmwareRevision",
    "isRemovable",
    "partitionTable",
    "partitionCount",
    "serial",
];

const GPU_KEYS: [&str; 17] = [
    "manufacturer",
    "model",
    "kind",
    "vendorId",
    "vendorIdHex",
    "deviceId",
    "deviceIdHex",
    "subsystemId",
    "revision",
    "vramMb",
    "sharedMemoryMb",
    "driverVersion",
    "driverDate",
    "pciBus",
    "currentResolution",
    "api",
    "uuid",
];

const GPU_API_KEYS: [&str; 15] = [
    "vulkan",
    "vulkanVersion",
    "vulkanDriver",
    "cuda",
    "cudaVersion",
    "computeCapability",
    "hip",
    "hipVersion",
    "rocmVersion",
    "gfxArchitecture",
    "directxFeatureLevel",
    "metal",
    "opencl",
    "openclVersion",
    "openglVersion",
];

fn assert_shape<T: serde::Serialize>(value: &T, expected: &[&str], label: &str) {
    let json = serde_json::to_value(value).expect("serializable");
    let object = json
        .as_object()
        .unwrap_or_else(|| panic!("{label} did not serialize to a JSON object"));
    let mut actual: Vec<&str> = object.keys().map(String::as_str).collect();
    let mut wanted: Vec<&str> = expected.to_vec();
    actual.sort_unstable();
    wanted.sort_unstable();
    assert_eq!(
        actual, wanted,
        "{label} JSON shape drifted from the contract"
    );
}

#[test]
fn disk_entries_serialize_the_contract_shape() {
    let info = scan_at(DetailLevel::Full, &[Section::Storage]);
    for disk in info.storage.iter().flat_map(|s| &s.disks) {
        assert_shape(disk, &DISK_KEYS, "disk");
    }
}

#[test]
fn gpu_entries_serialize_the_contract_shape() {
    let info = scan_at(DetailLevel::Capabilities, &[Section::Gpu]);
    for gpu in info.gpu.iter().flatten() {
        assert_shape(gpu, &GPU_KEYS, "gpu");
        assert_shape(&gpu.api, &GPU_API_KEYS, "gpu.api");
    }
}
