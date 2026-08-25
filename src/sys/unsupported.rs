//! Fallback for targets without a native backend.
//!
//! The portable collectors still run; only the platform-specific extras are
//! missing, and every caller is told why.

use std::collections::HashMap;

use super::{CpuNative, DisplayNative, MemoryNative, NetNative, OsNative};
use crate::models::*;
use crate::scan::Ctx;

fn unsupported(ctx: &mut Ctx, what: &str) {
    ctx.warn(format!(
        "{what}: no native backend for `{}`; only portable data is available",
        std::env::consts::OS
    ));
}

pub fn cpu(ctx: &mut Ctx) -> Vec<CpuNative> {
    unsupported(ctx, "cpu");
    Vec::new()
}

pub fn gpus(ctx: &mut Ctx) -> Vec<Gpu> {
    unsupported(ctx, "gpu");
    Vec::new()
}

pub fn memory(ctx: &mut Ctx) -> MemoryNative {
    unsupported(ctx, "memory modules");
    MemoryNative::default()
}

pub fn disks(ctx: &mut Ctx) -> Vec<Disk> {
    unsupported(ctx, "disks");
    Vec::new()
}

pub fn network(ctx: &mut Ctx) -> HashMap<String, NetNative> {
    unsupported(ctx, "network");
    HashMap::new()
}

pub fn displays(ctx: &mut Ctx) -> Vec<DisplayNative> {
    unsupported(ctx, "displays");
    Vec::new()
}

pub fn board(ctx: &mut Ctx) -> Board {
    unsupported(ctx, "board");
    Board {
        manufacturer: None,
        product: None,
        version: None,
        serial: None,
        asset_tag: None,
        bios: Bios::default(),
        chassis: Chassis::default(),
        system: SystemIdentity::default(),
    }
}

pub fn os(ctx: &mut Ctx) -> OsNative {
    unsupported(ctx, "os");
    OsNative::default()
}

pub fn hip(_ctx: &mut Ctx) -> crate::scan::compute::Hip {
    // ROCm targets Linux and Windows only; its absence elsewhere is expected
    // rather than a failed probe, so this stays quiet.
    crate::scan::compute::Hip::default()
}
