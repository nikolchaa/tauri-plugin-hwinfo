use sysinfo::Disks;

use super::{clean, to_mb, Ctx};
use crate::models::*;
use crate::sys;

pub fn collect(ctx: &mut Ctx) -> Storage {
    // Mounted volumes come free from `sysinfo` and answer the common question
    // - how much space is left. Enumerating the physical drives behind them
    // costs WMI queries or a helper process, so it waits for the full tier.
    let mut disks = sys::disks(ctx);
    if !ctx.mode.is_unsafe() {
        for d in &mut disks {
            d.serial = None;
        }
    }

    let volumes = Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|d| {
            let total = d.total_space();
            let available = d.available_space();
            Volume {
                mount_point: d.mount_point().to_string_lossy().into_owned(),
                name: clean(d.name().to_string_lossy()),
                file_system: clean(d.file_system().to_string_lossy()),
                total_mb: to_mb(total),
                available_mb: to_mb(available),
                used_mb: to_mb(total.saturating_sub(available)),
                is_removable: d.is_removable(),
                is_read_only: d.is_read_only(),
            }
        })
        .collect();

    Storage { disks, volumes }
}
