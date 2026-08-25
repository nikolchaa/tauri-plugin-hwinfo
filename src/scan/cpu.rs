//! CPU collection.
//!
//! Three sources are merged, most specific last:
//!
//! 1. `sysinfo` - logical processor list, live frequency, live load.
//! 2. `CPUID` - vendor, brand, family/model/stepping, cache geometry, ISA
//!    extensions, hypervisor identity. x86 only, but exact where available.
//! 3. The platform backend - socket, physical core count, rated clocks,
//!    microcode, package temperature.

use sysinfo::{CpuRefreshKind, RefreshKind, System};

use super::{clean, Ctx};
use crate::models::*;
use crate::sys;

pub fn collect(ctx: &mut Ctx) -> Vec<Cpu> {
    // Load is a delta between two samples, so measuring it costs a sampling
    // interval - the single largest fixed cost in this section. Below the full
    // tier the frequency alone is read, which is instantaneous.
    let with_load = ctx.wants(DetailLevel::Full);
    let refresh = if with_load {
        CpuRefreshKind::everything()
    } else {
        CpuRefreshKind::nothing().with_frequency()
    };

    let mut sys_info = System::new_with_specifics(RefreshKind::nothing().with_cpu(refresh));
    if with_load {
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        sys_info.refresh_cpu_all();
    }

    let logical: Vec<CpuCore> = sys_info
        .cpus()
        .iter()
        .map(|c| CpuCore {
            id: c.name().to_string(),
            usage_percent: with_load.then(|| c.cpu_usage()),
            frequency: (c.frequency() > 0).then_some(c.frequency() as u32),
        })
        .collect();

    let portable = Portable::new(&sys_info, with_load);
    let cpuid = CpuIdFacts::probe();
    let natives = sys::cpu(ctx);

    let package_count = natives.len().max(1);
    let cores_per_package = if package_count > 1 && logical.len() >= package_count {
        logical.len() / package_count
    } else {
        logical.len()
    };
    if package_count > 1 {
        ctx.warn(
            "multi-socket system: logical processors are split evenly across packages, which may \
             not match the true topology",
        );
    }

    (0..package_count)
        .map(|i| {
            let native = natives.get(i).cloned().unwrap_or_default();
            let cores: Vec<CpuCore> = logical
                .iter()
                .skip(i * cores_per_package)
                .take(cores_per_package)
                .cloned()
                .collect();

            build(ctx, native, &portable, cpuid.as_ref(), cores, package_count)
        })
        .collect()
}

fn build(
    ctx: &Ctx,
    native: sys::CpuNative,
    portable: &Portable,
    cpuid: Option<&CpuIdFacts>,
    cores: Vec<CpuCore>,
    package_count: usize,
) -> Cpu {
    let threads = native
        .threads
        .or_else(|| (!cores.is_empty()).then_some(cores.len() as u32))
        .unwrap_or_else(|| (portable.logical_count / package_count as u32).max(1));

    let physical_cores = native.physical_cores.or_else(|| {
        // `physical_core_count` is machine-wide; only trust it for one package.
        (package_count == 1)
            .then_some(portable.physical_cores)
            .flatten()
    });

    let current_frequency = native.current_frequency.or_else(|| {
        let live: Vec<u32> = cores.iter().filter_map(|c| c.frequency).collect();
        (!live.is_empty()).then(|| live.iter().sum::<u32>() / live.len() as u32)
    });

    // No platform reliably advertises the turbo ceiling: Windows' MaxClockSpeed
    // and CPUID leaf 0x16 both hand back the base clock on modern Intel parts.
    // Take the highest of everything advertised and everything observed, so a
    // core caught boosting corrects a too-low advertised figure rather than
    // being contradicted by it.
    let max_frequency = [
        native.max_frequency,
        cpuid.and_then(|c| c.max_frequency),
        cores.iter().filter_map(|c| c.frequency).max(),
        current_frequency,
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(0);

    let cache = CpuCache {
        l1d_kb: native.l1d_kb.or_else(|| cpuid.and_then(|c| c.cache.l1d_kb)),
        l1i_kb: native.l1i_kb.or_else(|| cpuid.and_then(|c| c.cache.l1i_kb)),
        l2_kb: native.l2_kb.or_else(|| cpuid.and_then(|c| c.cache.l2_kb)),
        l3_kb: native.l3_kb.or_else(|| cpuid.and_then(|c| c.cache.l3_kb)),
    };

    let sampled: Vec<f32> = cores.iter().filter_map(|c| c.usage_percent).collect();
    let usage = if sampled.is_empty() {
        (package_count == 1)
            .then_some(portable.global_usage)
            .flatten()
    } else {
        Some(sampled.iter().sum::<f32>() / sampled.len() as f32)
    };

    // The per-processor list is free to compute - it comes out of the same
    // refresh as the package figures - but it scales with the core count, and
    // on a large server it is by far the biggest thing in the payload. Callers
    // opt into carrying it.
    let cores = if ctx.wants(DetailLevel::Full) {
        cores
    } else {
        Vec::new()
    };

    Cpu {
        manufacturer: native
            .manufacturer
            .or_else(|| cpuid.and_then(|c| c.vendor.clone()))
            .or_else(|| portable.vendor_id.clone())
            .unwrap_or_else(|| "Unknown".into()),
        model: native
            .model
            .or_else(|| cpuid.and_then(|c| c.brand.clone()))
            .or_else(|| portable.brand.clone())
            .unwrap_or_else(|| "Unknown".into()),
        architecture: portable.architecture.clone(),
        physical_cores,
        threads,
        base_frequency: native
            .base_frequency
            .or_else(|| cpuid.and_then(|c| c.base_frequency)),
        max_frequency,
        current_frequency,
        socket: native.socket,
        family: cpuid.and_then(|c| c.family),
        model_id: cpuid.and_then(|c| c.model_id),
        stepping: cpuid.and_then(|c| c.stepping),
        microcode: native.microcode,
        cache,
        features: cpuid.map(|c| c.features.clone()).unwrap_or_default(),
        // Either source seeing the extensions is enough. Firmware flags read
        // false inside a VM and on hosts where a hypervisor has claimed them,
        // while CPUID can be masked; neither absence is proof.
        virtualization: match (native.virtualization, cpuid.and_then(|c| c.virtualization)) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(false) || b.unwrap_or(false)),
        },
        hypervisor: cpuid.and_then(|c| c.hypervisor.clone()),
        simultaneous_multithreading: physical_cores.map(|p| p > 0 && threads > p),
        usage_percent: usage,
        temperature_c: native.temperature_c.or(portable.temperature_c),
        cores,
        serial: ctx.redact(native.serial),
    }
}

/// What `sysinfo` reports, independent of platform.
struct Portable {
    vendor_id: Option<String>,
    brand: Option<String>,
    architecture: String,
    logical_count: u32,
    physical_cores: Option<u32>,
    global_usage: Option<f32>,
    temperature_c: Option<f32>,
}

impl Portable {
    fn new(sys_info: &System, with_load: bool) -> Self {
        let first = sys_info.cpus().first();
        Self {
            vendor_id: first.and_then(|c| clean(c.vendor_id())),
            brand: first.and_then(|c| clean(c.brand())),
            architecture: System::cpu_arch(),
            logical_count: sys_info.cpus().len() as u32,
            physical_cores: System::physical_core_count().map(|c| c as u32),
            global_usage: with_load.then(|| sys_info.global_cpu_usage()),
            // Enumerating sensors means another WMI namespace on Windows, so
            // it rides along with the rest of the live state.
            temperature_c: with_load.then(package_temperature).flatten(),
        }
    }
}

/// Pick the sensor that reports the package as a whole.
///
/// Names vary wildly by driver: `k10temp Tctl` on AMD, `coretemp Package id 0`
/// on Intel, `TC0P` on Intel Macs. Anything matching is better than nothing;
/// per-core sensors are used only if no package sensor exists.
fn package_temperature() -> Option<f32> {
    let components = sysinfo::Components::new_with_refreshed_list();
    let mut best: Option<f32> = None;

    for component in components.list() {
        let label = component.label().to_ascii_lowercase();
        let Some(temp) = component.temperature() else {
            continue;
        };
        if !temp.is_finite() || temp <= 0.0 {
            continue;
        }

        let is_package = label.contains("package")
            || label.contains("tctl")
            || label.contains("tdie")
            || label.contains("cpu");
        if is_package {
            return Some(temp);
        }
        if label.contains("core") {
            best = Some(best.map_or(temp, |b: f32| b.max(temp)));
        }
    }

    best
}

/// Facts read straight out of the `CPUID` instruction.
#[derive(Default)]
struct CpuIdFacts {
    vendor: Option<String>,
    brand: Option<String>,
    family: Option<u32>,
    model_id: Option<u32>,
    stepping: Option<u32>,
    base_frequency: Option<u32>,
    max_frequency: Option<u32>,
    virtualization: Option<bool>,
    hypervisor: Option<String>,
    cache: CpuCache,
    features: Vec<String>,
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
impl CpuIdFacts {
    fn probe() -> Option<Self> {
        None
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
impl CpuIdFacts {
    fn probe() -> Option<Self> {
        use raw_cpuid::{CacheType, CpuId, Hypervisor};

        let cpuid = CpuId::new();
        let mut out = CpuIdFacts {
            vendor: cpuid.get_vendor_info().and_then(|v| clean(v.as_str())),
            brand: cpuid
                .get_processor_brand_string()
                .and_then(|b| clean(b.as_str())),
            ..Default::default()
        };

        if let Some(f) = cpuid.get_feature_info() {
            out.family = Some(f.family_id() as u32);
            out.model_id = Some(f.model_id() as u32);
            out.stepping = Some(f.stepping_id() as u32);
            out.virtualization = Some(f.has_vmx());
            out.features = feature_names(&cpuid, &f);
        }

        // AMD reports hardware virtualisation as SVM rather than VMX.
        if out.virtualization != Some(true) {
            if let Some(e) = cpuid.get_extended_processor_and_feature_identifiers() {
                if e.has_svm() {
                    out.virtualization = Some(true);
                }
            }
        }

        if let Some(f) = cpuid.get_processor_frequency_info() {
            out.base_frequency =
                (f.processor_base_frequency() > 0).then_some(f.processor_base_frequency() as u32);
            out.max_frequency =
                (f.processor_max_frequency() > 0).then_some(f.processor_max_frequency() as u32);
        }

        out.hypervisor = cpuid.get_hypervisor_info().map(|h| {
            match h.identify() {
                Hypervisor::Xen => "Xen".into(),
                Hypervisor::VMware => "VMware".into(),
                Hypervisor::HyperV => "Microsoft Hyper-V".into(),
                Hypervisor::KVM => "KVM".into(),
                Hypervisor::QEMU => "QEMU".into(),
                Hypervisor::Bhyve => "bhyve".into(),
                Hypervisor::QNX => "QNX".into(),
                Hypervisor::ACRN => "ACRN".into(),
                Hypervisor::Unknown(a, b, c) => {
                    // The identity is a 12-byte ASCII string in three registers.
                    let mut raw = Vec::with_capacity(12);
                    for reg in [a, b, c] {
                        raw.extend_from_slice(&reg.to_le_bytes());
                    }
                    clean(String::from_utf8_lossy(&raw))
                        .unwrap_or_else(|| "Unknown hypervisor".into())
                }
            }
        });

        if let Some(caches) = cpuid.get_cache_parameters() {
            for cache in caches {
                // size = ways * partitions * line size * sets
                let size_kb = (cache.associativity() as u64
                    * cache.physical_line_partitions() as u64
                    * cache.coherency_line_size() as u64
                    * cache.sets() as u64
                    / 1024) as u32;
                if size_kb == 0 {
                    continue;
                }
                let slot = match (cache.level(), cache.cache_type()) {
                    (1, CacheType::Data) => &mut out.cache.l1d_kb,
                    (1, CacheType::Instruction) => &mut out.cache.l1i_kb,
                    (2, _) => &mut out.cache.l2_kb,
                    (3, _) => &mut out.cache.l3_kb,
                    _ => continue,
                };
                // Leaf 4 reports per-cache size; sum the instances at each level
                // so L1/L2 totals reflect the whole package.
                *slot = Some(slot.unwrap_or(0) + size_kb);
            }
        }

        Some(out)
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn feature_names(
    cpuid: &raw_cpuid::CpuId<raw_cpuid::CpuIdReaderNative>,
    f: &raw_cpuid::FeatureInfo,
) -> Vec<String> {
    let mut out: Vec<&str> = Vec::new();
    let mut add = |present: bool, name: &'static str| {
        if present {
            out.push(name);
        }
    };

    add(f.has_fpu(), "FPU");
    add(f.has_mmx(), "MMX");
    add(f.has_sse(), "SSE");
    add(f.has_sse2(), "SSE2");
    add(f.has_sse3(), "SSE3");
    add(f.has_ssse3(), "SSSE3");
    add(f.has_sse41(), "SSE4.1");
    add(f.has_sse42(), "SSE4.2");
    add(f.has_avx(), "AVX");
    add(f.has_fma(), "FMA3");
    add(f.has_aesni(), "AES-NI");
    add(f.has_pclmulqdq(), "PCLMULQDQ");
    add(f.has_popcnt(), "POPCNT");
    add(f.has_movbe(), "MOVBE");
    add(f.has_f16c(), "F16C");
    add(f.has_rdrand(), "RDRAND");
    add(f.has_cmpxchg16b(), "CX16");
    add(f.has_xsave(), "XSAVE");
    add(f.has_vmx(), "VT-x");
    add(f.has_smx(), "SMX");
    add(f.has_tsc(), "TSC");
    add(f.has_htt(), "HTT");
    add(f.has_pcid(), "PCID");
    add(f.has_x2apic(), "X2APIC");
    add(f.has_hypervisor(), "HYPERVISOR");

    if let Some(e) = cpuid.get_extended_feature_info() {
        add(e.has_avx2(), "AVX2");
        add(e.has_avx512f(), "AVX-512F");
        add(e.has_avx512dq(), "AVX-512DQ");
        add(e.has_avx512bw(), "AVX-512BW");
        add(e.has_avx512vl(), "AVX-512VL");
        add(e.has_avx512cd(), "AVX-512CD");
        add(e.has_avx512_ifma(), "AVX-512IFMA");
        add(e.has_avx512vbmi(), "AVX-512VBMI");
        add(e.has_avx512vbmi2(), "AVX-512VBMI2");
        add(e.has_avx512vnni(), "AVX-512VNNI");
        add(e.has_avx512bitalg(), "AVX-512BITALG");
        add(e.has_avx512vpopcntdq(), "AVX-512VPOPCNTDQ");
        add(e.has_avx512_bf16(), "AVX-512BF16");
        add(e.has_avx512_fp16(), "AVX-512FP16");
        add(e.has_avx_vnni(), "AVX-VNNI");
        add(e.has_avx_ifma(), "AVX-IFMA");
        add(e.has_avx_ne_convert(), "AVX-NE-CONVERT");
        add(e.has_avx_vnni_int8(), "AVX-VNNI-INT8");
        add(e.has_avx_vnni_int16(), "AVX-VNNI-INT16");
        add(e.has_avx10(), "AVX10");
        add(e.has_amx_tile(), "AMX-TILE");
        add(e.has_amx_int8(), "AMX-INT8");
        add(e.has_amx_bf16(), "AMX-BF16");
        add(e.has_bmi1(), "BMI1");
        add(e.has_bmi2(), "BMI2");
        add(e.has_adx(), "ADX");
        add(e.has_sha(), "SHA");
        add(e.has_gfni(), "GFNI");
        add(e.has_vaes(), "VAES");
        add(e.has_vpclmulqdq(), "VPCLMULQDQ");
        add(e.has_rdseed(), "RDSEED");
        add(e.has_rdpid(), "RDPID");
        add(e.has_fsgsbase(), "FSGSBASE");
        add(e.has_smep(), "SMEP");
        add(e.has_smap(), "SMAP");
        add(e.has_sgx(), "SGX");
        add(e.has_mpx(), "MPX");
        add(e.has_hle(), "HLE");
        add(e.has_rtm(), "RTM");
        add(e.has_pku(), "PKU");
        add(e.has_la57(), "LA57");
        add(e.has_clflushopt(), "CLFLUSHOPT");
        add(e.has_clwb(), "CLWB");
        add(e.has_cet_ss(), "CET-SS");
        add(e.has_waitpkg(), "WAITPKG");
        add(e.has_invpcid(), "INVPCID");
        add(e.has_processor_trace(), "INTEL-PT");
    }

    if let Some(e) = cpuid.get_extended_processor_and_feature_identifiers() {
        add(e.has_64bit_mode(), "LM");
        add(e.has_svm(), "AMD-V");
        add(e.has_lzcnt(), "LZCNT");
        add(e.has_sse4a(), "SSE4A");
        add(e.has_xop(), "XOP");
        add(e.has_fma4(), "FMA4");
        add(e.has_tbm(), "TBM");
        add(e.has_prefetchw(), "PREFETCHW");
        add(e.has_rdtscp(), "RDTSCP");
        add(e.has_1gib_pages(), "PDPE1GB");
        add(e.has_execute_disable(), "NX");
        add(e.has_syscall_sysret(), "SYSCALL");
        add(e.has_3dnow(), "3DNOW");
        add(e.has_monitorx_mwaitx(), "MONITORX");
    }

    let mut names: Vec<String> = out.into_iter().map(str::to_string).collect();
    names.sort_unstable();
    names.dedup();
    names
}
