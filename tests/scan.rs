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
        assert!(battery.serial.is_none(), "battery serial leaked in safe mode");
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
        // The observed peak can only ever raise the advertised maximum.
        if let Some(current) = cpu.current_frequency {
            assert!(cpu.max_frequency >= current, "max clock below current clock");
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

    assert_eq!(info.scan.version, 2);
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
