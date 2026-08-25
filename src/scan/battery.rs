use super::Ctx;
use crate::models::*;

// `starship-battery` has no implementation for Android or iOS and fails to
// build there outright, so the cargo feature alone is not enough to enable
// this collector.

#[cfg(not(all(
    feature = "battery",
    any(
        target_os = "windows",
        target_os = "linux",
        target_os = "macos",
        target_os = "ios"
    )
)))]
pub fn collect(ctx: &mut Ctx) -> Vec<Battery> {
    #[cfg(feature = "battery")]
    ctx.warn(format!(
        "battery: no power-supply backend for `{}`",
        std::env::consts::OS
    ));
    #[cfg(not(feature = "battery"))]
    ctx.warn("battery: the `battery` cargo feature is disabled");

    Vec::new()
}

#[cfg(all(
    feature = "battery",
    any(
        target_os = "windows",
        target_os = "linux",
        target_os = "macos",
        target_os = "ios"
    )
))]
pub fn collect(ctx: &mut Ctx) -> Vec<Battery> {
    use starship_battery::units::{
        electric_potential::volt, energy::watt_hour, power::watt, ratio::percent,
        thermodynamic_temperature::degree_celsius, time::second,
    };
    use starship_battery::{Manager, State};

    let manager = match Manager::new() {
        Ok(m) => m,
        Err(e) => {
            ctx.warn(format!("battery: {e}"));
            return Vec::new();
        }
    };

    let batteries = match manager.batteries() {
        Ok(b) => b,
        Err(e) => {
            ctx.warn(format!("battery: {e}"));
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    for entry in batteries {
        let b = match entry {
            Ok(b) => b,
            Err(e) => {
                ctx.warn(format!("battery: {e}"));
                continue;
            }
        };

        let full = b.energy_full().get::<watt_hour>();
        let design = b.energy_full_design().get::<watt_hour>();

        out.push(Battery {
            vendor: b.vendor().and_then(super::clean),
            model: b.model().and_then(super::clean),
            technology: super::clean(b.technology().to_string()),
            state: match b.state() {
                State::Charging => BatteryState::Charging,
                State::Discharging => BatteryState::Discharging,
                State::Empty => BatteryState::Empty,
                State::Full => BatteryState::Full,
                _ => BatteryState::Unknown,
            },
            charge_percent: b.state_of_charge().get::<percent>(),
            // `state_of_health` is reported as 0 when the pack does not expose a
            // design capacity; derive it ourselves rather than claiming 0% health.
            health_percent: (design > 0.0).then(|| (full / design * 100.0).min(100.0)),
            energy_wh: Some(b.energy().get::<watt_hour>()),
            energy_full_wh: (full > 0.0).then_some(full),
            energy_full_design_wh: (design > 0.0).then_some(design),
            energy_rate_w: Some(match b.state() {
                State::Discharging => -b.energy_rate().get::<watt>(),
                _ => b.energy_rate().get::<watt>(),
            }),
            voltage_v: Some(b.voltage().get::<volt>()),
            temperature_c: b.temperature().map(|t| t.get::<degree_celsius>()),
            cycle_count: b.cycle_count(),
            seconds_to_full: b.time_to_full().map(|t| t.get::<second>() as u64),
            seconds_to_empty: b.time_to_empty().map(|t| t.get::<second>() as u64),
            serial: ctx.redact(b.serial_number().and_then(super::clean)),
        });
    }

    out
}
