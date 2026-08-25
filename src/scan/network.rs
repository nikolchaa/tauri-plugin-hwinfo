use sysinfo::{InterfaceOperationalState, Networks};

use super::Ctx;
use crate::models::*;
use crate::sys;

pub fn collect(ctx: &mut Ctx) -> Vec<NetworkInterface> {
    let native = sys::network(ctx);
    let networks = Networks::new_with_refreshed_list();

    let mut out: Vec<NetworkInterface> = networks
        .list()
        .iter()
        .map(|(name, data)| {
            let extra = native.get(name).cloned().unwrap_or_default();
            let mac =
                (!data.mac_address().is_unspecified()).then(|| data.mac_address().to_string());
            let ips: Vec<String> = data.ip_networks().iter().map(|n| n.to_string()).collect();

            NetworkInterface {
                name: name.clone(),
                description: extra.description,
                state: state_name(data.operational_state()).to_string(),
                mtu: (data.mtu() > 0).then_some(data.mtu()),
                speed_mbps: extra.speed_mbps,
                total_received: data.total_received(),
                total_transmitted: data.total_transmitted(),
                packets_received: data.total_packets_received(),
                packets_transmitted: data.total_packets_transmitted(),
                errors_received: data.total_errors_on_received(),
                errors_transmitted: data.total_errors_on_transmitted(),
                mac_address: ctx.redact(mac),
                ip_networks: ctx.redact((!ips.is_empty()).then_some(ips)),
            }
        })
        .collect();

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn state_name(state: InterfaceOperationalState) -> &'static str {
    match state {
        InterfaceOperationalState::Up => "up",
        InterfaceOperationalState::Down => "down",
        InterfaceOperationalState::Testing => "testing",
        InterfaceOperationalState::Dormant => "dormant",
        InterfaceOperationalState::NotPresent => "notPresent",
        InterfaceOperationalState::LowerLayerDown => "lowerLayerDown",
        InterfaceOperationalState::Unknown => "unknown",
        _ => "unknown",
    }
}
