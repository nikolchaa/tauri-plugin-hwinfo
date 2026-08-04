//! Displays through Core Graphics rather than `system_profiler`.
//!
//! `CGDisplay*` is the interface macOS itself uses to describe attached
//! screens. It is faster than a `system_profiler` spawn, and — more
//! importantly — it returns typed scalars instead of a JSON document whose key
//! names Apple has renamed several times across releases.
//!
//! The declarations below are hand-written rather than pulled from a bindings
//! crate: every one of these functions takes and returns plain C scalars or
//! `#[repr(C)]` structs of two `f64`s, and the set has been stable for the
//! life of the 64-bit platform.

use std::ffi::c_void;

use super::super::util::pnp_vendor;
use super::super::DisplayNative;
use crate::scan::{clean, Ctx};
use crate::sys::util::edid_vendor_code;

type CGDirectDisplayID = u32;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetActiveDisplayList(
        max_displays: u32,
        active_displays: *mut CGDirectDisplayID,
        display_count: *mut u32,
    ) -> i32;
    fn CGMainDisplayID() -> CGDirectDisplayID;
    fn CGDisplayVendorNumber(display: CGDirectDisplayID) -> u32;
    fn CGDisplayModelNumber(display: CGDirectDisplayID) -> u32;
    fn CGDisplaySerialNumber(display: CGDirectDisplayID) -> u32;
    fn CGDisplayScreenSize(display: CGDirectDisplayID) -> CGSize;
    fn CGDisplayPixelsWide(display: CGDirectDisplayID) -> usize;
    fn CGDisplayPixelsHigh(display: CGDirectDisplayID) -> usize;
    fn CGDisplayBounds(display: CGDirectDisplayID) -> CGRect;
    fn CGDisplayIsBuiltin(display: CGDirectDisplayID) -> i32;
    /// Returns a retained `CGDisplayMode`, which the caller must release.
    fn CGDisplayCopyDisplayMode(display: CGDirectDisplayID) -> *mut c_void;
    fn CGDisplayModeGetRefreshRate(mode: *mut c_void) -> f64;
    fn CGDisplayModeGetPixelWidth(mode: *mut c_void) -> usize;
    fn CGDisplayModeGetPixelHeight(mode: *mut c_void) -> usize;
    fn CGDisplayModeRelease(mode: *mut c_void);
}

/// Sentinel Core Graphics uses when a display reports no identifier.
const UNKNOWN_ID: u32 = 0xFFFF_FFFF;

pub fn displays(ctx: &mut Ctx) -> Vec<DisplayNative> {
    const MAX_DISPLAYS: u32 = 32;

    let mut ids = [0u32; MAX_DISPLAYS as usize];
    let mut count: u32 = 0;

    let status = unsafe { CGGetActiveDisplayList(MAX_DISPLAYS, ids.as_mut_ptr(), &mut count) };
    if status != 0 {
        ctx.warn(format!(
            "display: CGGetActiveDisplayList failed with error {status}"
        ));
        return Vec::new();
    }

    let main = unsafe { CGMainDisplayID() };

    ids.iter()
        .take(count as usize)
        .map(|&id| build(id, main))
        .collect()
}

fn build(id: CGDirectDisplayID, main: CGDirectDisplayID) -> DisplayNative {
    // SAFETY: `id` came from CGGetActiveDisplayList, so it names a live
    // display; all of these take it by value and return scalars.
    let (bounds, physical, builtin) = unsafe {
        (
            CGDisplayBounds(id),
            CGDisplayScreenSize(id),
            CGDisplayIsBuiltin(id) != 0,
        )
    };

    // The current mode carries the refresh rate and the true pixel dimensions,
    // which differ from the point dimensions on a Retina panel.
    let mut refresh_rate_hz = None;
    let mut native_width = None;
    let mut native_height = None;
    let mode = unsafe { CGDisplayCopyDisplayMode(id) };
    if !mode.is_null() {
        // SAFETY: non-null means Core Graphics handed us a retained mode.
        unsafe {
            // Built-in panels and many external ones report 0, meaning the
            // refresh is not variable rather than that it is zero.
            let rate = CGDisplayModeGetRefreshRate(mode);
            if rate > 0.0 {
                refresh_rate_hz = Some((rate * 100.0).round() / 100.0);
            }
            native_width = Some(CGDisplayModeGetPixelWidth(mode) as u32);
            native_height = Some(CGDisplayModeGetPixelHeight(mode) as u32);
            CGDisplayModeRelease(mode);
        }
    }

    let vendor = unsafe { CGDisplayVendorNumber(id) };
    let product = unsafe { CGDisplayModelNumber(id) };
    let serial = unsafe { CGDisplaySerialNumber(id) };

    let usable = |value: u32| value != 0 && value != UNKNOWN_ID;

    // EDID gives a numeric product code, not a marketing name. Built-in panels
    // are better described by what they are; for anything else the code is
    // still more useful than nothing.
    let model = if builtin {
        Some("Built-in Display".to_string())
    } else {
        usable(product).then(|| format!("Display {product:04X}"))
    };

    DisplayNative {
        // Core Graphics has no name for a display, only identifiers. The
        // portable merge fills this from the window system.
        name: None,
        manufacturer: usable(vendor)
            .then(|| edid_vendor_code(vendor as u16))
            .flatten()
            .map(|code| pnp_vendor(&code).map(str::to_string).unwrap_or(code)),
        model,
        width: Some(unsafe { CGDisplayPixelsWide(id) } as u32),
        height: Some(unsafe { CGDisplayPixelsHigh(id) } as u32),
        refresh_rate_hz,
        native_width,
        native_height,
        position_x: Some(bounds.origin.x as i32),
        position_y: Some(bounds.origin.y as i32),
        is_primary: Some(id == main),
        is_internal: Some(builtin),
        // Core Graphics does not report colour depth; every modern Mac is
        // 32-bit or deeper and the field is not worth guessing at.
        bits_per_pixel: None,
        physical_width_mm: (physical.width > 0.0).then_some(physical.width.round() as u32),
        physical_height_mm: (physical.height > 0.0).then_some(physical.height.round() as u32),
        // Core Graphics exposes no manufacture date.
        manufacture_year: None,
        serial: usable(serial)
            .then(|| clean(serial.to_string()))
            .flatten(),
    }
}
