//! Minimal EDID 1.x decoder.
//!
//! Only the fields this plugin reports are decoded: manufacturer, model name,
//! serial, physical size and year of manufacture. Anything malformed is
//! skipped rather than guessed at.

use crate::scan::clean;
use crate::sys::util::{edid_vendor_code, pnp_vendor};
use crate::sys::DisplayNative;

const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
/// Base EDID blocks are always exactly 128 bytes; extensions follow.
const BLOCK_LEN: usize = 128;

/// Fill in whatever `bytes` can supply, leaving other fields untouched.
pub fn apply(bytes: &[u8], display: &mut DisplayNative) {
    if bytes.len() < BLOCK_LEN || bytes[..8] != HEADER {
        return;
    }

    display.manufacturer =
        manufacturer_id(bytes).map(|id| pnp_vendor(&id).map(str::to_string).unwrap_or(id));

    // Physical size lives in bytes 21-22 as whole centimetres. Both being zero
    // means the display is a projector or did not declare a size.
    let width_cm = bytes[21] as u32;
    let height_cm = bytes[22] as u32;
    if width_cm > 0 && height_cm > 0 {
        display.physical_width_mm = Some(width_cm * 10);
        display.physical_height_mm = Some(height_cm * 10);
    }

    // Byte 17 holds the year of manufacture as an offset from 1990.
    if bytes[17] > 0 {
        display.manufacture_year = Some(1990 + bytes[17] as u32);
    }

    for descriptor in descriptors(bytes) {
        // A descriptor starting with two zero bytes is a text block; byte 3
        // says which kind.
        if descriptor[0] != 0 || descriptor[1] != 0 {
            continue;
        }
        let text = || clean(String::from_utf8_lossy(&descriptor[5..18]).replace('\n', " "));
        match descriptor[3] {
            0xFC => display.model = text(),
            0xFF => display.serial = text(),
            _ => {}
        }
    }
}

/// The three-letter vendor code packed into bytes 8-9.
fn manufacturer_id(bytes: &[u8]) -> Option<String> {
    edid_vendor_code(u16::from_be_bytes([bytes[8], bytes[9]]))
}

/// The four 18-byte descriptor blocks at the end of the base block.
fn descriptors(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    (0..4).filter_map(move |i| {
        let start = 54 + i * 18;
        bytes.get(start..start + 18)
    })
}
