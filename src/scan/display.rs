//! Display collection.
//!
//! Tauri's monitor list is the portable baseline - it is the only source of the
//! DPI scale factor and it agrees with what the app's own windows see. The
//! platform backend supplies refresh rate, colour depth and EDID identity.
//!
//! The two lists are matched by adapter name, then by position, then by index.

use super::{Ctx, MonitorHint};
use crate::models::*;
use crate::sys::util::diagonal_inches;
use crate::sys::{self, DisplayNative};

pub fn collect(ctx: &mut Ctx) -> Vec<Display> {
    let mut natives = sys::displays(ctx);
    let monitors = ctx.monitors.clone();

    if monitors.is_empty() && natives.is_empty() {
        ctx.warn("display: no monitors could be enumerated");
        return Vec::new();
    }

    let mut out: Vec<Display> = Vec::new();

    for (index, monitor) in monitors.iter().enumerate() {
        let native = take_match(&mut natives, monitor, index);
        out.push(merge(ctx, Some(monitor), native.unwrap_or_default()));
    }

    // Displays the window system knows about but Tauri did not report - for
    // example a monitor attached to a second adapter that has no windows on it.
    for native in natives {
        out.push(merge(ctx, None, native));
    }

    out
}

/// Pull the native entry that corresponds to `monitor` out of the pool.
fn take_match(
    natives: &mut Vec<DisplayNative>,
    monitor: &MonitorHint,
    index: usize,
) -> Option<DisplayNative> {
    let by_name = monitor.name.as_deref().and_then(|name| {
        natives
            .iter()
            .position(|n| n.name.as_deref().is_some_and(|c| c == name))
    });

    let by_position = || {
        natives.iter().position(|n| {
            n.position_x == Some(monitor.position_x) && n.position_y == Some(monitor.position_y)
        })
    };

    let pos = by_name
        .or_else(by_position)
        .or_else(|| (index < natives.len()).then_some(index))?;

    Some(natives.remove(pos))
}

fn merge(ctx: &Ctx, monitor: Option<&MonitorHint>, native: DisplayNative) -> Display {
    let width = monitor
        .map(|m| m.width)
        .or(native.width)
        .unwrap_or_default();
    let height = monitor
        .map(|m| m.height)
        .or(native.height)
        .unwrap_or_default();

    let native_resolution = match (native.native_width, native.native_height) {
        (Some(w), Some(h)) if w != width || h != height => Some(Resolution {
            width: w,
            height: h,
            refresh_rate_hz: None,
        }),
        _ => None,
    };

    Display {
        name: native
            .name
            .clone()
            .or_else(|| monitor.and_then(|m| m.name.clone())),
        manufacturer: native.manufacturer,
        model: native.model,
        resolution: Resolution {
            width,
            height,
            refresh_rate_hz: native.refresh_rate_hz,
        },
        native_resolution,
        scale_factor: monitor.map(|m| m.scale_factor).unwrap_or(1.0),
        position_x: monitor
            .map(|m| m.position_x)
            .or(native.position_x)
            .unwrap_or_default(),
        position_y: monitor
            .map(|m| m.position_y)
            .or(native.position_y)
            .unwrap_or_default(),
        is_primary: monitor
            .map(|m| m.is_primary)
            .or(native.is_primary)
            .unwrap_or(false),
        is_internal: native.is_internal,
        bits_per_pixel: native.bits_per_pixel,
        physical_width_mm: native.physical_width_mm,
        physical_height_mm: native.physical_height_mm,
        diagonal_inches: native
            .physical_width_mm
            .zip(native.physical_height_mm)
            .and_then(|(w, h)| diagonal_inches(w, h)),
        manufacture_year: native.manufacture_year,
        serial: ctx.redact(native.serial),
    }
}
