//! Small helpers shared by the platform backends.

#![allow(dead_code)]

use std::process::Command;

/// Run a helper binary and return its stdout, or `Err` with a short reason.
///
/// On Windows the child is created with `CREATE_NO_WINDOW` so a packaged app
/// never flashes a console.
pub fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("could not run `{program}`: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "`{program}` exited with {}",
            output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "a signal".into())
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Read a file, trimming trailing whitespace. Used all over `/sys` and `/proc`.
pub fn read_trimmed(path: impl AsRef<std::path::Path>) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Read a file containing a single integer.
pub fn read_u64(path: impl AsRef<std::path::Path>) -> Option<u64> {
    read_trimmed(path)?.parse().ok()
}

/// `"key: value"` → `value`, for line-oriented tool output.
pub fn value_after_colon(line: &str) -> Option<&str> {
    line.split_once(':').map(|(_, v)| v.trim())
}

/// Decode a UTF-16 buffer that may or may not be NUL-terminated.
#[cfg(windows)]
pub fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len]).trim().to_string()
}

/// Diagonal size in inches from physical dimensions in millimetres.
pub fn diagonal_inches(width_mm: u32, height_mm: u32) -> Option<f64> {
    if width_mm == 0 || height_mm == 0 {
        return None;
    }
    let w = width_mm as f64;
    let h = height_mm as f64;
    let inches = (w * w + h * h).sqrt() / 25.4;
    Some((inches * 10.0).round() / 10.0)
}

/// Decode the 16-bit EDID manufacturer field into its three-letter code.
///
/// The three letters are packed five bits each, `A` = 1, with the top bit
/// unused. macOS surfaces the same value as a decimal number, so both backends
/// arrive here.
pub fn edid_vendor_code(packed: u16) -> Option<String> {
    if packed == 0 {
        return None;
    }
    let letter = |shift: u16| {
        let value = ((packed >> shift) & 0x1F) as u8;
        (1..=26).contains(&value).then(|| (b'A' + value - 1) as char)
    };
    Some([letter(10)?, letter(5)?, letter(0)?].into_iter().collect())
}

/// Spell out a three-letter EDID/PnP vendor code.
///
/// The full registry runs to thousands of entries and ships as a separate data
/// file on most systems; this covers the panel and monitor makers a desktop app
/// is realistically going to meet. Unknown codes are returned to the caller
/// as-is rather than dropped.
pub fn pnp_vendor(code: &str) -> Option<&'static str> {
    Some(match code.to_ascii_uppercase().as_str() {
        "AAP" | "APP" => "Apple Inc.",
        "ACI" => "ASUSTeK Computer Inc.",
        "ACR" => "Acer Technologies",
        "AOC" => "AOC International",
        "AUO" => "AU Optronics",
        "BNQ" => "BenQ Corporation",
        "BOE" => "BOE Technology Group",
        "CMN" => "Chi Mei Innolux",
        "DEL" => "Dell Inc.",
        "GSM" => "LG Electronics",
        "HPN" | "HWP" => "HP Inc.",
        "HSD" => "HannStar Display",
        "IVM" => "Iiyama",
        "LEN" => "Lenovo Group Limited",
        "LGD" => "LG Display",
        "MSI" => "Micro-Star International",
        "NEC" => "NEC Corporation",
        "PHL" => "Philips",
        "SAM" => "Samsung Electronics",
        "SDC" => "Samsung Display",
        "SHP" => "Sharp Corporation",
        "SNY" => "Sony Corporation",
        "VSC" => "ViewSonic Corporation",
        _ => return None,
    })
}
