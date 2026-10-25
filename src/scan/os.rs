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

    // On mobile `sysinfo` answers with the wrong thing entirely: the handset
    // model (`ro.product.model`) on Android and the kernel type ("Darwin") on
    // iOS. `os_info` names the OS itself there.
    let name = if cfg!(any(target_os = "android", target_os = "ios")) {
        clean(info.os_type().to_string())
    } else {
        clean_opt(System::name()).or_else(|| clean(info.os_type().to_string()))
    };

    Os {
        name: name.unwrap_or_else(|| family.to_string()),
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
