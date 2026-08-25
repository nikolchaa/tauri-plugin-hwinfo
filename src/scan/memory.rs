use sysinfo::{MemoryRefreshKind, RefreshKind, System};

use super::{to_mb, Ctx};
use crate::models::*;
use crate::sys;

pub fn collect(ctx: &mut Ctx) -> Memory {
    let sys_info =
        System::new_with_specifics(RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()));

    let native = sys::memory(ctx);
    let mut modules = native.modules;
    if !ctx.mode.is_unsafe() {
        for m in &mut modules {
            m.serial = None;
        }
    }

    Memory {
        total_mb: to_mb(sys_info.total_memory()),
        available_mb: to_mb(sys_info.available_memory()),
        used_mb: to_mb(sys_info.used_memory()),
        swap_total_mb: to_mb(sys_info.total_swap()),
        swap_used_mb: to_mb(sys_info.used_swap()),
        slots_used: native
            .slots_used
            .or_else(|| (!modules.is_empty()).then_some(modules.len() as u32)),
        slots_total: native.slots_total,
        modules,
    }
}
