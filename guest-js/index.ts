import { invoke } from "@tauri-apps/api/core";

export interface CpuInfo {
  manufacturer: string;
  model: string;
  maxFrequency: number;
  threads: number;
}

export interface RamInfo {
  sizeMb: number;
}

export interface GpuInfo {
  manufacturer: string;
  model: string;
  vramMb: number;
}

export interface OsInfo {
  name: string;
  version: string;
}

// Plugin commands

export async function getCpuInfo(): Promise<CpuInfo> {
  return await invoke("plugin:hwinfo|get_cpu_info");
}

export async function getRamInfo(): Promise<RamInfo> {
  return await invoke("plugin:hwinfo|get_ram_info");
}

export async function getGpuInfo(): Promise<GpuInfo> {
  return await invoke("plugin:hwinfo|get_gpu_info");
}

export async function getOsInfo(): Promise<OsInfo> {
  return await invoke("plugin:hwinfo|get_os_info");
}
