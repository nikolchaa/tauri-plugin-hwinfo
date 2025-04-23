use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    pub manufacturer: String, // e.g. "AMD"
    pub model: String, // e.g. "AMD Ryzen 9 5900X 12-Core Processor"
    pub max_frequency: u32, // e.g. 3700
    pub threads: usize, // e.g. 24
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RamInfo {
    pub size_mb: u64, // e.g. 32768
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub manufacturer: String, // e.g. "Advanced Micro Devices, Inc."
    pub model: String, // e.g. "AMD Radeon RX 6950 XT"
    pub vram_mb: u64, // e.g. 16384

    pub supports_cuda: bool, // e.g. false
    pub supports_vulkan: bool, // e.g. true
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OsInfo {
    pub name: String, // e.g. "Windows"
    pub version: String, // e.g. "10.0.19045"
}
