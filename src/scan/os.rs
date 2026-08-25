use sysinfo::System;

use super::{clean, clean_opt, Ctx};
use crate::models::*;
use crate::sys;

pub fn collect(ctx: &mut Ctx) -> Os {
    let native = sys::os(ctx);
    let info = os_info::get();

    let family = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        std::env::consts::OS
    };

    // `os_info` knows the marketing edition; `sysinfo` knows what the kernel
    // says. Prefer whichever is more specific for each field.
    let version = clean(info.version().to_string())
        .or_else(|| clean_opt(System::os_version()))
        .unwrap_or_else(|| "Unknown".into());

    Os {
        name: clean_opt(System::name())
            .or_else(|| clean(info.os_type().to_string()))
            .unwrap_or_else(|| family.to_string()),
        family: family.to_string(),
        version,
        long_version: clean_opt(System::long_os_version()),
        kernel_version: clean_opt(System::kernel_version()),
        build: native.build,
        edition: native.edition.or_else(|| clean_opt(info.edition())),
        codename: native.codename.or_else(|| clean_opt(info.codename())),
        distribution_id: clean(System::distribution_id()),
        architecture: System::cpu_arch(),
        uptime_secs: System::uptime(),
        boot_time_secs: System::boot_time(),
        virtualization: native.virtualization,
        hostname: ctx.redact(clean_opt(System::host_name())),
        machine_id: ctx.redact(native.machine_id),
        user: ctx.redact(native.user),
    }
}
