//! Pins the serde output of the models to the fixtures in `tests/contract/`.
//!
//! `tests/contract/check.mjs` type-checks the same fixtures against
//! `guest-js/index.ts`, so between them the Rust structs and the hand-written
//! TypeScript types can't drift apart unnoticed. The fixtures fill in every
//! optional field and every list, which a real scan on a CI runner can't: a
//! `null` or an empty array there says nothing about the type behind it.

use std::{fs, path::Path};

use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use tauri_plugin_hwinfo::{
    BatteryState, DetailLevel, DiskKind, GpuKind, ScanMode, ScanOptions, Section, SystemInfo,
};

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/contract")
        .join(name);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Collects every path where the fixture and the models disagree.
fn diff(path: &str, fixture: &Value, models: &Value, out: &mut Vec<String>) {
    match (fixture, models) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                let path = format!("{path}.{key}");
                match b.get(key) {
                    Some(other) => diff(&path, value, other, out),
                    None => out.push(format!("{path}: in the fixture, not in models.rs")),
                }
            }
            for key in b.keys().filter(|key| !a.contains_key(*key)) {
                out.push(format!("{path}.{key}: in models.rs, not in the fixture"));
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                diff(&format!("{path}[{i}]"), x, y, out);
            }
        }
        _ if fixture != models => {
            out.push(format!(
                "{path}: fixture has {fixture}, models.rs gives {models}"
            ));
        }
        _ => {}
    }
}

fn assert_no_diff(name: &str, fixture: &Value, models: &Value) {
    let mut diffs = Vec::new();
    diff(name, fixture, models, &mut diffs);
    assert!(
        diffs.is_empty(),
        "tests/contract/{name}.json no longer matches src/models.rs. Update the \
         fixture and guest-js/index.ts together:\n  {}",
        diffs.join("\n  ")
    );
}

/// Deserialises the fixture as `T` and serialises it again. A key the structs
/// lack is dropped and an optional one the fixture lacks comes back `null`,
/// so a rename or a new field on either side shows up in the diff.
fn assert_round_trips<T: Serialize + DeserializeOwned>(name: &str) {
    let expected = fixture(&format!("{name}.json"));
    let value: T = serde_json::from_value(expected.clone())
        .unwrap_or_else(|e| panic!("tests/contract/{name}.json no longer deserialises: {e}"));
    // Through a string rather than `to_value`, so an `f32` compares as the
    // decimal the frontend receives instead of its widened `f64`.
    let actual = serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
    assert_no_diff(name, &expected, &actual);
}

/// A `null` or an empty list leaves the TypeScript check nothing to type.
fn assert_fully_populated(path: &str, value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Null => out.push(format!("{path} is null")),
        Value::Array(items) if items.is_empty() => out.push(format!("{path} is empty")),
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                assert_fully_populated(&format!("{path}[{i}]"), item, out);
            }
        }
        Value::Object(fields) => {
            for (key, field) in fields {
                assert_fully_populated(&format!("{path}.{key}"), field, out);
            }
        }
        _ => {}
    }
}

#[test]
fn fixtures_match_the_models() {
    assert_round_trips::<SystemInfo>("SystemInfo");
    assert_round_trips::<ScanOptions>("ScanOptions");
}

#[test]
fn fixtures_fill_every_field() {
    for name in ["SystemInfo", "ScanOptions"] {
        let mut gaps = Vec::new();
        assert_fully_populated(name, &fixture(&format!("{name}.json")), &mut gaps);
        assert!(
            gaps.is_empty(),
            "fill these in tests/contract/{name}.json:\n  {}",
            gaps.join("\n  ")
        );
    }
}

/// Serialises every variant of an enum. The `match` has no wildcard arm, so
/// adding a variant stops this compiling until it is listed.
macro_rules! variants {
    ($ty:ident: $($variant:ident),+ $(,)?) => {{
        let _exhaustive = |value: $ty| match value {
            $($ty::$variant)|+ => {}
        };
        json!([$($ty::$variant),+])
    }};
}

#[test]
fn enum_fixture_lists_every_variant() {
    let models = json!({
        "ScanMode": variants!(ScanMode: Safe, Unsafe),
        "DetailLevel": variants!(DetailLevel: Summary, Capabilities, Full),
        "Section": variants!(
            Section: Cpu, Gpu, Memory, Storage, Network, Display, Battery, Board, Os,
        ),
        "GpuKind": variants!(GpuKind: Discrete, Integrated, Virtual, Cpu, Unknown),
        "DiskKind": variants!(DiskKind: Hdd, Ssd, Unknown),
        "BatteryState": variants!(BatteryState: Charging, Discharging, Empty, Full, Unknown),
    });
    assert_no_diff("enums", &fixture("enums.json"), &models);
}
