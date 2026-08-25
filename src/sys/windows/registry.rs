//! Registry and firmware reads.
//!
//! A handful of facts live only here: the update-build revision, the marketing
//! release label, the Secure Boot state and the machine GUID.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};
use windows::Win32::System::SystemInformation::{
    FirmwareTypeBios, FirmwareTypeUefi, GetFirmwareType, FIRMWARE_TYPE,
};

use crate::scan::clean;

const CURRENT_VERSION: PCWSTR = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
const CENTRAL_PROCESSOR: PCWSTR = w!("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0");

/// Read a string value under `HKLM`.
fn read_string(subkey: PCWSTR, value: PCWSTR) -> Option<String> {
    let mut size: u32 = 0;
    // First call sizes the buffer.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS || size == 0 {
        return None;
    }

    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }

    clean(crate::sys::util::from_wide(&buffer))
}

/// Read a `REG_DWORD` under `HKLM`.
fn read_u32(subkey: PCWSTR, value: PCWSTR) -> Option<u32> {
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            RRF_RT_REG_DWORD,
            None,
            Some(std::ptr::from_mut(&mut data).cast()),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(data)
}

/// A string value under `Windows NT\CurrentVersion`.
pub fn current_version(value: &str) -> Option<String> {
    let name = wide(value);
    read_string(CURRENT_VERSION, PCWSTR(name.as_ptr()))
}

/// A DWORD value under `Windows NT\CurrentVersion`.
pub fn current_version_u32(value: &str) -> Option<u32> {
    let name = wide(value);
    read_u32(CURRENT_VERSION, PCWSTR(name.as_ptr()))
}

/// Read a `REG_BINARY` value under `HKLM`.
fn read_binary(subkey: PCWSTR, value: PCWSTR) -> Option<Vec<u8>> {
    let mut size: u32 = 0;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            RRF_RT_REG_BINARY,
            None,
            None,
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS || size == 0 {
        return None;
    }

    let mut buffer = vec![0u8; size as usize];
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            RRF_RT_REG_BINARY,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then(|| {
        buffer.truncate(size as usize);
        buffer
    })
}

/// The running microcode revision.
///
/// `Update Revision` is a little-endian binary blob whose width varies by
/// Windows build: eight bytes with the revision in the upper `u32` on older
/// systems, four bytes holding it directly on newer ones. Read from logical
/// processor 0 - every core on a machine runs the same microcode.
pub fn microcode_revision() -> Option<String> {
    let raw = read_binary(CENTRAL_PROCESSOR, w!("Update Revision"))?;
    let dword = |offset: usize| {
        raw.get(offset..offset + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .filter(|&v| v != 0)
    };

    let revision = dword(4).or_else(|| dword(0))?;
    Some(format!("0x{revision:08X}"))
}

pub fn machine_guid() -> Option<String> {
    read_string(w!("SOFTWARE\\Microsoft\\Cryptography"), w!("MachineGuid"))
}

/// Secure Boot state. `None` on a legacy-BIOS machine, where the key is absent.
pub fn secure_boot_enabled() -> Option<bool> {
    read_u32(
        w!("SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State"),
        w!("UEFISecureBootEnabled"),
    )
    .map(|v| v == 1)
}

pub fn firmware_type() -> Option<String> {
    let mut kind = FIRMWARE_TYPE::default();
    unsafe { GetFirmwareType(&mut kind) }.ok()?;
    // windows-rs models these as newtype constants rather than enum variants,
    // so they are compared, not matched.
    if kind == FirmwareTypeUefi {
        Some("UEFI".into())
    } else if kind == FirmwareTypeBios {
        Some("Legacy".into())
    } else {
        None
    }
}

/// A NUL-terminated UTF-16 copy of `value`.
///
/// The caller binds the result before taking a pointer to it, so the buffer
/// outlives the `PCWSTR` that borrows it.
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
