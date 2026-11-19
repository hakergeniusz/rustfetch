//! x86 CPUID helpers, ported from fastfetch's detection/cpu/cpu.c and
//! cpu_x86.c: the frequency/march leaf 0x16+1+7 detection and the libcpuid
//! codename matcher over the tables in cpu_tables.rs.

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::__cpuid;

use crate::cpu_tables::{self, MatchEntry};
use crate::modules::cpu::CpuResult;

/// Vendor strings as CPUID leaf 0 spells them.
const VENDOR_INTEL: &[u8] = b"GenuineIntel";
const VENDOR_AMD: &[u8] = b"AuthenticAMD";
const VENDOR_CENTAUR: &[u8] = b"CentaurHauls";

/// The highest standard CPUID leaf, or None when CPUID is unavailable.
#[cfg(target_arch = "x86_64")]
fn max_leaf() -> Option<u32> {
    // EFER/CR0 checks are overkill: CPUID always executes on x86_64.
    let result = __cpuid(0);
    Some(result.eax)
}

/// XCR0 via XGETBV; only called when OSXSAVE is set, so it never faults.
#[cfg(target_arch = "x86_64")]
fn xgetbv() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        std::arch::asm!(
            "xgetbv",
            out("rax") low,
            out("rdx") high,
            in("rcx") 0,
            options(nomem, nostack)
        );
    }
    ((high as u64) << 32) | low as u64
}

/// ffCPUDetectByCpuid: base/max frequencies from leaf 0x16 and the
/// x86-64 microarchitecture level from leaves 1 and 7. Note the upstream
/// code masks ECX with bit_SSE2 (EDX's bit in the spec); ported literally.
#[cfg(target_arch = "x86_64")]
pub fn detect_by_cpuid(cpu: &mut CpuResult) {
    let Some(max) = max_leaf() else { return };

    if max >= 0x16 {
        // cpuid returns 0 MHz when a hypervisor is enabled.
        let leaf16 = __cpuid(0x16);
        if leaf16.eax != 0 {
            cpu.frequency_base = leaf16.eax;
        }
        if leaf16.ebx != 0 {
            cpu.frequency_max = leaf16.ebx;
        }
    }

    if max < 1 {
        return;
    }
    let leaf1 = __cpuid(1);
    let ecx = leaf1.ecx;
    let sse2 = (ecx >> 26) & 1 != 0;
    let sse4_2 = (ecx >> 20) & 1 != 0;
    let pclmul = (ecx >> 1) & 1 != 0;
    let popcnt = (ecx >> 23) & 1 != 0;
    let fma = (ecx >> 12) & 1 != 0;
    let osxsave = (ecx >> 27) & 1 != 0;

    let ebx7 = if max >= 7 {
        __cpuid(7).ebx
    } else {
        0
    };
    let avx2 = (ebx7 >> 5) & 1 != 0;
    let bmi2 = (ebx7 >> 8) & 1 != 0;
    let avx512f = (ebx7 >> 16) & 1 != 0;
    let avx512bw = (ebx7 >> 30) & 1 != 0;
    let avx512dq = (ebx7 >> 17) & 1 != 0;

    let mut avx_os = false;
    let mut avx512_os = false;
    if osxsave {
        let xcr0 = xgetbv();
        // AVX needs XCR0[1:2], AVX512 needs XCR0 bits 1,2,5,6,7.
        avx_os = (xcr0 & 0x6) == 0x6;
        avx512_os = (xcr0 & 0xE6) == 0xE6;
    }

    cpu.march = Some("unknown");
    if avx512f && avx512bw && avx512dq && avx512_os {
        cpu.march = Some("x86_64-v4");
    } else if avx2 && fma && bmi2 && avx_os {
        cpu.march = Some("x86_64-v3");
    } else if sse4_2 && popcnt && pclmul {
        cpu.march = Some("x86_64-v2");
    } else if sse2 {
        cpu.march = Some("x86_64-v1");
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub fn detect_by_cpuid(_cpu: &mut CpuResult) {}

/// The 12-byte vendor string from CPUID leaf 0 (ebx, edx, ecx order).
#[cfg(target_arch = "x86_64")]
pub fn cpuid_vendor() -> Option<String> {
    let result = __cpuid(0);
    let mut vendor = [0u8; 12];
    vendor[0..4].copy_from_slice(&result.ebx.to_le_bytes());
    vendor[4..8].copy_from_slice(&result.edx.to_le_bytes());
    vendor[8..12].copy_from_slice(&result.ecx.to_le_bytes());
    Some(String::from_utf8_lossy(&vendor).into_owned())
}

/// match_pattern_core: '#' matches a digit, '[abc]' matches one of the
/// chars, everything else literally; the brand must end after the pattern.
fn match_pattern_core(pattern: &[u8], brand: &[u8]) -> bool {
    let mut p = 0;
    let mut b = 0;
    while p < pattern.len() && b < brand.len() {
        match pattern[p] {
            b'#' => {
                if !brand[b].is_ascii_digit() {
                    return false;
                }
                p += 1;
                b += 1;
            },
            b'[' => {
                p += 1;
                let mut found = false;
                while p < pattern.len() && pattern[p] != b']' {
                    if pattern[p] == brand[b] {
                        found = true;
                    }
                    p += 1;
                }
                if !found {
                    return false;
                }
                if p < pattern.len() {
                    p += 1; // ']'
                }
                b += 1;
            },
            c => {
                if c != brand[b] {
                    return false;
                }
                p += 1;
                b += 1;
            },
        }
    }
    // ignore suffix
    p == pattern.len()
}

/// match_brand_pattern: the pattern may match anywhere in the brand.
fn match_brand_pattern(pattern: &str, brand: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    if brand.is_empty() {
        return false;
    }
    brand
        .bytes()
        .enumerate()
        .any(|(i, _)| match_pattern_core(pattern.as_bytes(), &brand.as_bytes()[i..]))
}

/// ffCPUDetectX86Specific: fill the vendor when empty, then match the
/// libcpuid tables for the codename and process technology.
#[cfg(target_arch = "x86_64")]
pub fn detect_x86_specific(cpu: &mut CpuResult) -> bool {
    if cpu.vendor.is_empty()
        && let Some(vendor) = cpuid_vendor()
    {
        cpu.vendor = vendor;
    }

    let Some(max) = max_leaf() else { return false };
    if max < 1 {
        return false;
    }
    let eax = __cpuid(1).eax;

    let family = ((eax >> 8) & 0xF) as i32;
    let model = ((eax >> 4) & 0xF) as i32;
    let stepping = (eax & 0xF) as i32;
    let xfamily = ((eax >> 20) & 0xFF) as i32;

    let ext_family = if family < 0xF && cpu.vendor.as_bytes() == VENDOR_AMD {
        family
    } else {
        family + xfamily
    };

    let xmodel = ((eax >> 16) & 0xF) as i32;
    let ext_model = model + (xmodel << 4);

    let table: &[MatchEntry] = if cpu.vendor.as_bytes() == VENDOR_INTEL {
        cpu_tables::CPUDB_INTEL
    } else if cpu.vendor.as_bytes() == VENDOR_AMD {
        cpu_tables::CPUDB_AMD
    } else if cpu.vendor.as_bytes() == VENDOR_CENTAUR {
        cpu_tables::CPUDB_CENTAUR
    } else {
        return false;
    };

    let mut best: Option<&MatchEntry> = None;
    let mut best_score = -1;
    for entry in table {
        if entry.family != family {
            continue;
        }
        if entry.model == model {
            // +2
        } else if entry.model != -1 {
            continue;
        }
        if entry.stepping == stepping {
            // +2
        } else if entry.stepping != -1 {
            continue;
        }
        if entry.ext_family == ext_family {
            // +2
        } else if entry.ext_family != -1 {
            continue;
        }
        if entry.ext_model == ext_model {
            // +2
        } else if entry.ext_model != -1 {
            continue;
        }
        let mut score = 0;
        for (value, expect) in [
            (entry.model, model),
            (entry.stepping, stepping),
            (entry.ext_family, ext_family),
            (entry.ext_model, ext_model),
        ] {
            if value == expect {
                score += 2;
            }
        }
        if !entry.pattern.is_empty() {
            if match_brand_pattern(entry.pattern, &cpu.name) {
                score += entry.score;
            } else {
                continue;
            }
        }
        if score > best_score {
            best_score = score;
            best = Some(entry);
        }
    }

    match best {
        Some(entry) if best_score > 0 => {
            cpu.code_name = Some(entry.name);
            cpu.technology = Some(entry.technology);
            true
        },
        _ => false,
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub fn detect_x86_specific(_cpu: &mut CpuResult) -> bool {
    false
}
