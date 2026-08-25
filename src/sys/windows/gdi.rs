//! Display enumeration through GDI.
//!
//! `EnumDisplayDevicesW` walks adapters, then the monitor attached to each one.
//! The monitor's device path carries the EDID PnP identifier, which is the key
//! used to look the panel up in `ROOT\WMI`.

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, EnumDisplaySettingsW, DEVMODEW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ACTIVE,
    DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_MIRRORING_DRIVER,
    DISPLAY_DEVICE_PRIMARY_DEVICE, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE,
    ENUM_REGISTRY_SETTINGS,
};

use crate::scan::Ctx;
use crate::sys::util::from_wide;
use crate::sys::DisplayNative;

/// A display plus the key used to join it to the WMI monitor tables.
pub struct Found {
    pub display: DisplayNative,
    pub monitor_key: Option<String>,
}

/// Ask `EnumDisplayDevicesW` for the monitor child's device interface path
/// rather than its friendly name.
const EDD_GET_DEVICE_INTERFACE_NAME: u32 = 0x0000_0001;

pub fn displays(ctx: &mut Ctx) -> Vec<Found> {
    let mut out = Vec::new();

    for index in 0.. {
        let mut adapter = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };

        let ok = unsafe { EnumDisplayDevicesW(PCWSTR::null(), index, &mut adapter, 0) };
        if !ok.as_bool() {
            break;
        }

        let flags = adapter.StateFlags.0;
        // Mirroring drivers are software pseudo-devices, and an adapter with no
        // desktop attached has no display to report.
        if flags & DISPLAY_DEVICE_MIRRORING_DRIVER.0 != 0
            || flags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP.0 == 0
        {
            continue;
        }

        let device_name = from_wide(&adapter.DeviceName);
        let name_wide: Vec<u16> = adapter
            .DeviceName
            .iter()
            .copied()
            .take_while(|&c| c != 0)
            .chain(std::iter::once(0))
            .collect();
        let name_ptr = PCWSTR(name_wide.as_ptr());

        let current = settings(name_ptr, ENUM_CURRENT_SETTINGS);
        // The registry settings hold the mode the panel was configured with,
        // which is the native mode for anything that has not been downscaled.
        let native = settings(name_ptr, ENUM_REGISTRY_SETTINGS);

        let mut display = DisplayNative {
            name: Some(device_name),
            is_primary: Some(flags & DISPLAY_DEVICE_PRIMARY_DEVICE.0 != 0),
            ..Default::default()
        };

        if let Some(mode) = current {
            display.width = Some(mode.dmPelsWidth);
            display.height = Some(mode.dmPelsHeight);
            display.refresh_rate_hz =
                (mode.dmDisplayFrequency > 1).then_some(mode.dmDisplayFrequency as f64);
            display.bits_per_pixel = (mode.dmBitsPerPel > 0).then_some(mode.dmBitsPerPel);
            // SAFETY: the position union member is valid for display devices,
            // which is what we enumerated.
            let position = unsafe { mode.Anonymous1.Anonymous2.dmPosition };
            display.position_x = Some(position.x);
            display.position_y = Some(position.y);
        }

        if let Some(mode) = native {
            display.native_width = Some(mode.dmPelsWidth);
            display.native_height = Some(mode.dmPelsHeight);
        }

        out.push(Found {
            display,
            monitor_key: monitor_key(name_ptr),
        });
    }

    if out.is_empty() {
        ctx.warn("display: EnumDisplayDevices returned no attached adapters");
    }

    out
}

fn settings(device: PCWSTR, which: ENUM_DISPLAY_SETTINGS_MODE) -> Option<DEVMODEW> {
    let mut mode = DEVMODEW {
        dmSize: std::mem::size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    let ok = unsafe { EnumDisplaySettingsW(device, which, &mut mode) };
    ok.as_bool().then_some(mode)
}

/// The EDID identity of the monitor attached to `adapter`, normalised to match
/// a WMI `InstanceName`.
fn monitor_key(adapter: PCWSTR) -> Option<String> {
    let mut monitor = DISPLAY_DEVICEW {
        cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };

    let ok =
        unsafe { EnumDisplayDevicesW(adapter, 0, &mut monitor, EDD_GET_DEVICE_INTERFACE_NAME) };
    if !ok.as_bool() || monitor.StateFlags.0 & DISPLAY_DEVICE_ACTIVE.0 == 0 {
        return None;
    }

    let id = from_wide(&monitor.DeviceID);
    (!id.is_empty()).then(|| super::normalise_instance(&id))
}
