//! SMBIOS structures read straight from `/sys/firmware/dmi/entries`.
//!
//! This replaces shelling out to `dmidecode`. It needs the same privileges —
//! the `raw` files are mode `0400`, which is a deliberate kernel decision
//! because these tables carry serial numbers — but it drops the dependency on
//! `dmidecode` being installed, avoids a process spawn, and parses the binary
//! structures rather than re-parsing that tool's English output.
//!
//! Field offsets follow the SMBIOS specification. Structures grow over time by
//! appending, and the header's `length` byte says how far the current firmware
//! actually went, so every read past the 2.0 core is bounds-checked against it.

use std::path::Path;

use crate::scan::clean;

const ENTRIES: &str = "/sys/firmware/dmi/entries";

/// One decoded SMBIOS structure: its fixed-size formatted area plus the string
/// table that follows.
pub struct Structure {
    formatted: Vec<u8>,
    strings: Vec<String>,
}

impl Structure {
    /// Split a raw structure into its formatted area and string table.
    fn parse(raw: &[u8]) -> Option<Self> {
        // Header is type, length, handle; `length` covers the formatted area
        // including the header itself.
        let length = *raw.get(1)? as usize;
        if length < 4 || raw.len() < length {
            return None;
        }

        // The string table is NUL-separated and ends with an empty string.
        let mut strings = Vec::new();
        let mut rest = &raw[length..];
        while let Some(end) = rest.iter().position(|&b| b == 0) {
            if end == 0 {
                break;
            }
            strings.push(String::from_utf8_lossy(&rest[..end]).into_owned());
            rest = &rest[end + 1..];
        }

        Some(Self {
            formatted: raw[..length].to_vec(),
            strings,
        })
    }

    /// A byte from the formatted area, or `None` if this firmware's structure
    /// is too short to include it.
    pub fn byte(&self, offset: usize) -> Option<u8> {
        self.formatted.get(offset).copied()
    }

    /// A little-endian `u16`.
    pub fn word(&self, offset: usize) -> Option<u16> {
        let bytes = self.formatted.get(offset..offset + 2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// A little-endian `u32`.
    pub fn dword(&self, offset: usize) -> Option<u32> {
        let bytes = self.formatted.get(offset..offset + 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Resolve a string reference. Index 0 means "not set".
    pub fn string(&self, offset: usize) -> Option<String> {
        let index = self.byte(offset)? as usize;
        if index == 0 {
            return None;
        }
        clean(self.strings.get(index - 1)?)
    }
}

/// Read every structure of a given SMBIOS type, in firmware order.
///
/// Returns an empty vector when the tables are unreadable, which on Linux
/// normally means the process is not root.
pub fn structures(kind: u8) -> Vec<Structure> {
    let Ok(entries) = std::fs::read_dir(ENTRIES) else {
        return Vec::new();
    };

    let prefix = format!("{kind}-");
    let mut matching: Vec<_> = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .strip_prefix(&prefix)
                // `17-0` must not also match `170-0`.
                .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()))
        })
        .collect();
    matching.sort_by_key(|e| {
        e.file_name()
            .to_string_lossy()
            .rsplit('-')
            .next()
            .and_then(|n| n.parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    });

    matching
        .into_iter()
        .filter_map(|entry| {
            let raw = std::fs::read(entry.path().join("raw")).ok()?;
            Structure::parse(&raw)
        })
        .collect()
}

/// Whether the DMI tables are readable at all by this process.
pub fn readable() -> bool {
    Path::new(ENTRIES).exists() && !structures(0).is_empty()
}

// ---------------------------------------------------------------------------
// Type 4 — Processor
// ---------------------------------------------------------------------------

pub struct Processor {
    pub socket: Option<String>,
    pub core_count: Option<u32>,
    pub thread_count: Option<u32>,
    pub max_speed_mhz: Option<u32>,
}

pub fn processors() -> Vec<Processor> {
    structures(4)
        .iter()
        .map(|s| Processor {
            socket: s.string(0x04),
            // Core and thread counts arrived in SMBIOS 2.5; 0xFF means "see
            // the 16-bit field", which only matters above 255 cores.
            core_count: s.byte(0x23).filter(|&v| v > 0 && v < 0xFF).map(u32::from),
            thread_count: s.byte(0x25).filter(|&v| v > 0 && v < 0xFF).map(u32::from),
            max_speed_mhz: s.word(0x14).filter(|&v| v > 0).map(u32::from),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Type 16 — Physical Memory Array
// ---------------------------------------------------------------------------

/// Total DIMM slots on the board, summed across arrays.
pub fn memory_slots() -> Option<u32> {
    let arrays = structures(16);
    if arrays.is_empty() {
        return None;
    }
    let total: u32 = arrays
        .iter()
        .filter_map(|s| s.word(0x0D))
        .map(u32::from)
        .sum();
    (total > 0).then_some(total)
}

// ---------------------------------------------------------------------------
// Type 17 — Memory Device
// ---------------------------------------------------------------------------

pub struct MemoryDevice {
    pub locator: Option<String>,
    pub bank_locator: Option<String>,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub serial: Option<String>,
    pub capacity_mb: Option<u64>,
    pub speed_mts: Option<u32>,
    pub configured_speed_mts: Option<u32>,
    pub memory_type: Option<&'static str>,
    pub form_factor: Option<&'static str>,
    pub voltage_mv: Option<u32>,
    pub rank: Option<u32>,
    pub data_width_bits: Option<u32>,
    pub total_width_bits: Option<u32>,
}

pub fn memory_devices() -> Vec<MemoryDevice> {
    structures(17)
        .iter()
        .filter_map(|s| {
            let capacity_mb = capacity(s);
            // An empty slot is reported with size zero.
            capacity_mb?;

            Some(MemoryDevice {
                locator: s.string(0x10),
                bank_locator: s.string(0x11),
                manufacturer: s.string(0x17),
                part_number: s.string(0x1A),
                serial: s.string(0x18),
                capacity_mb,
                speed_mts: s.word(0x15).filter(|&v| v > 0).map(u32::from),
                configured_speed_mts: s.word(0x20).filter(|&v| v > 0).map(u32::from),
                memory_type: s.byte(0x12).and_then(memory_type),
                form_factor: s.byte(0x0E).and_then(form_factor),
                // Configured voltage, in millivolts. Zero means unknown.
                voltage_mv: s.word(0x26).filter(|&v| v > 0).map(u32::from),
                // Rank lives in the low nibble of the attributes byte.
                rank: s.byte(0x1B).map(|a| u32::from(a & 0x0F)).filter(|&r| r > 0),
                data_width_bits: s.word(0x0A).filter(|&v| v != 0xFFFF).map(u32::from),
                total_width_bits: s.word(0x08).filter(|&v| v != 0xFFFF).map(u32::from),
            })
        })
        .collect()
}

/// Decode the size field, which is a tangle of special cases.
///
/// The 16-bit field uses bit 15 as a kilobyte flag; `0xFFFF` means unknown and
/// `0x7FFF` means "too large, see the 32-bit extended field".
fn capacity(s: &Structure) -> Option<u64> {
    let raw = s.word(0x0C)?;
    if raw == 0 || raw == 0xFFFF {
        return None;
    }
    if raw == 0x7FFF {
        // The extended field is in megabytes, with the top bit reserved.
        let extended = s.dword(0x1C)? & 0x7FFF_FFFF;
        return (extended > 0).then_some(u64::from(extended));
    }
    let value = u64::from(raw & 0x7FFF);
    if raw & 0x8000 != 0 {
        // Kilobytes: only ever seen on very small legacy modules.
        Some(value / 1024)
    } else {
        Some(value)
    }
}

/// SMBIOS 7.18.2 memory type codes.
///
/// These are the specification's own values, which differ from the CIM values
/// WMI reports for the same concept on Windows.
fn memory_type(code: u8) -> Option<&'static str> {
    Some(match code {
        0x03 => "DRAM",
        0x04 => "EDRAM",
        0x05 => "VRAM",
        0x06 => "SRAM",
        0x07 => "RAM",
        0x08 => "ROM",
        0x0F => "SDRAM",
        0x11 => "SDRAM",
        0x12 => "SGRAM",
        0x13 => "RDRAM",
        0x14 => "DDR",
        0x15 => "DDR2",
        0x16 => "DDR2 FB-DIMM",
        0x18 => "DDR3",
        0x19 => "FBD2",
        0x1A => "DDR4",
        0x1B => "LPDDR",
        0x1C => "LPDDR2",
        0x1D => "LPDDR3",
        0x1E => "LPDDR4",
        0x1F => "Logical non-volatile device",
        0x20 => "HBM",
        0x21 => "HBM2",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        0x24 => "HBM3",
        _ => return None,
    })
}

/// SMBIOS 7.18.1 form factor codes.
fn form_factor(code: u8) -> Option<&'static str> {
    Some(match code {
        0x03 => "SIMM",
        0x04 => "SIP",
        0x05 => "Chip",
        0x06 => "DIP",
        0x07 => "ZIP",
        0x08 => "Proprietary Card",
        0x09 => "DIMM",
        0x0A => "TSOP",
        0x0B => "Row of chips",
        0x0C => "RIMM",
        0x0D => "SODIMM",
        0x0E => "SRIMM",
        0x0F => "FB-DIMM",
        0x10 => "Die",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a structure the way firmware lays one out: header, formatted
    /// area, then a double-NUL-terminated string table.
    fn raw(kind: u8, formatted: &[u8], strings: &[&str]) -> Vec<u8> {
        let length = 4 + formatted.len();
        let mut out = vec![kind, length as u8, 0x01, 0x00];
        out.extend_from_slice(formatted);
        for s in strings {
            out.extend_from_slice(s.as_bytes());
            out.push(0);
        }
        out.push(0);
        out
    }

    #[test]
    fn resolves_strings_by_one_based_index() {
        // Two string references at offsets 4 and 5, pointing at entries 1 and 2.
        let structure = Structure::parse(&raw(17, &[1, 2, 0], &["Kingston", "DIMM A1"])).unwrap();
        assert_eq!(structure.string(4).as_deref(), Some("Kingston"));
        assert_eq!(structure.string(5).as_deref(), Some("DIMM A1"));
        // Index zero means "not set", not "first string".
        assert_eq!(structure.string(6), None);
    }

    #[test]
    fn reads_past_the_declared_length_return_none() {
        let structure = Structure::parse(&raw(4, &[0x01], &["AM5"])).unwrap();
        assert_eq!(structure.byte(4), Some(0x01));
        // Older firmware simply stops early; that must not be read as zero.
        assert_eq!(structure.byte(0x23), None);
        assert_eq!(structure.word(0x20), None);
    }

    #[test]
    fn decodes_capacity_special_cases() {
        // A plain 16 GiB module: megabytes, bit 15 clear.
        let plain =
            Structure::parse(&raw(17, &vec_at(0x0C, &16384u16.to_le_bytes()), &[])).unwrap();
        assert_eq!(capacity(&plain), Some(16384));

        // Anything at or above 32 GiB overflows the 15-bit field, so firmware
        // sets the sentinel and puts the real megabyte count at 0x1C.
        let mut formatted = vec_at(0x0C, &0x7FFFu16.to_le_bytes());
        let ext = 0x1C - 4;
        formatted[ext..ext + 4].copy_from_slice(&32768u32.to_le_bytes());
        let large = Structure::parse(&raw(17, &formatted, &[])).unwrap();
        assert_eq!(capacity(&large), Some(32768));

        // Bit 15 set means the value is in kilobytes.
        let kb = Structure::parse(&raw(
            17,
            &vec_at(0x0C, &(0x8000u16 | 2048).to_le_bytes()),
            &[],
        ))
        .unwrap();
        assert_eq!(capacity(&kb), Some(2));

        // Unknown and empty slots yield nothing rather than zero.
        let unknown =
            Structure::parse(&raw(17, &vec_at(0x0C, &0xFFFFu16.to_le_bytes()), &[])).unwrap();
        assert_eq!(capacity(&unknown), None);
        let empty = Structure::parse(&raw(17, &vec_at(0x0C, &0u16.to_le_bytes()), &[])).unwrap();
        assert_eq!(capacity(&empty), None);
    }

    /// A formatted area with `bytes` placed at absolute `offset`.
    fn vec_at(offset: usize, bytes: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; offset - 4];
        out.extend_from_slice(bytes);
        // Pad out to cover the extended-size field so `dword` reads succeed.
        out.resize(0x24, 0);
        out
    }
}
