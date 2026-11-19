//! `cpu` module: full CPU detection, ported from detection/cpu/cpu.c,
//! cpu_linux.c (x86 path) and cpu_x86.c. The JSON result exposes all
//! fields; the text value is "name (cores) @ X GHz".

use crate::{
    cpu_x86,
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

#[derive(Default)]
pub struct CpuResult {
    pub name: String,
    pub vendor: String,
    pub packages: u16,
    pub cores_physical: u16,
    pub cores_logical: u16,
    pub cores_online: u16,
    pub numa_nodes: u16,
    pub frequency_base: u32,
    pub frequency_max: u32,
    /// (count, freq MHz) pairs, like the upstream 16-entry array.
    pub core_types: Vec<(u32, u32)>,
    /// The --temp option isn't exposed by this CLI (upstream defaults it
    /// off), so the temperature is always unset.
    pub temperature: Option<f64>,
    pub march: Option<&'static str>,
    pub code_name: Option<&'static str>,
    pub technology: Option<&'static str>,
}



/// Count the CPUs in a sysfs range list like "0-3,8-11".
fn count_cpu_list(path: &str) -> Option<u16> {
    let content = util::read_file(path)?;
    let mut count = 0u16;
    for range in content.trim().split(',') {
        let range = range.trim();
        match range.split_once('-') {
            Some((low, high)) => {
                let (low, high) = (low.trim().parse::<u32>().ok()?, high.trim().parse::<u32>().ok()?);
                count += (high - low + 1) as u16;
            },
            None => count += range.parse::<u32>().ok()?.min(1) as u16,
        }
    }
    Some(count)
}

/// get_nprocs_conf / get_nprocs via the sysfs cpu lists, falling back to
/// the cpuinfo processor lines.
fn processor_counts() -> (u16, u16) {
    let fallback = || -> u16 {
        util::read_file("/proc/cpuinfo")
            .map(|content| {
                content
                    .lines()
                    .filter(|line| {
                        util::parse_colon_line(line).is_some_and(|(key, _)| key == "processor")
                    })
                    .count() as u16
            })
            .unwrap_or(0)
    };
    (
        count_cpu_list("/sys/devices/system/cpu/present").unwrap_or_else(fallback),
        count_cpu_list("/sys/devices/system/cpu/online").unwrap_or_else(fallback),
    )
}

/// getPackageCount: the number of distinct "physical id" values.
fn package_count(cpuinfo: &str) -> u16 {
    let mut ids = std::collections::HashSet::new();
    for line in cpuinfo.lines() {
        if let Some((key, value)) = util::parse_colon_line(line)
            && key == "physical id"
            && let Ok(id) = value.trim().parse::<u32>()
        {
            ids.insert(id);
        }
    }
    ids.len() as u16
}

/// parseCpuInfo (x86 branch): name, vendor, "cpu cores", "cpu MHz" from
/// the first processor block, stopping at its trailing blank line.
fn parse_cpu_info(
    cpuinfo: &str,
) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    let mut name = None;
    let mut vendor = None;
    let mut cores = None;
    let mut mhz = None;
    for line in cpuinfo.lines() {
        if line.is_empty() && name.is_some() {
            break;
        }
        if let Some((key, value)) = util::parse_colon_line(line) {
            match key {
                "model name" if name.is_none() => name = Some(value.to_string()),
                "vendor_id" if vendor.is_none() => vendor = Some(value.to_string()),
                "cpu cores" if cores.is_none() => cores = Some(value.to_string()),
                "cpu MHz" if mhz.is_none() => mhz = Some(value.to_string()),
                _ => {},
            }
        }
    }
    (name, vendor, cores, mhz)
}

/// getFrequency: kHz sysfs value → MHz, cpuinfo_ preferred over scaling_.
fn get_frequency(policy: &std::path::Path, cpuinfo_file: &str, scaling_file: Option<&str>) -> u32 {
    if let Some(value) = util::read_trimmed_in(policy, cpuinfo_file)
        && let Ok(khz) = value.trim().parse::<u32>()
    {
        return khz / 1000;
    }
    if let Some(scaling_file) = scaling_file
        && let Some(value) = util::read_trimmed_in(policy, scaling_file)
        && let Ok(khz) = value.trim().parse::<u32>()
    {
        return khz / 1000;
    }
    0
}

/// detectFrequency: the cpufreq policies with their max/base frequencies
/// and, with showPeCoreCount on (the default), the per-frequency core
/// counts from affected_cpus.
fn detect_frequency(cpu: &mut CpuResult) -> bool {
    let Ok(dir) = std::fs::read_dir("/sys/devices/system/cpu/cpufreq/") else {
        return false;
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("policy")
            || !name["policy".len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        {
            continue;
        }
        let policy = entry.path();

        let fmax = get_frequency(&policy, "cpuinfo_max_freq", Some("scaling_max_freq"));
        if fmax == 0 {
            continue;
        }

        // With showPeCoreCount on, every policy contributes its core count
        // even when its max frequency is not the new maximum.
        if cpu.frequency_max < fmax {
            cpu.frequency_max = fmax;
        }

        let fbase = get_frequency(&policy, "base_frequency", None);
        if fbase > 0 {
            cpu.frequency_base = cpu.frequency_base.max(fbase);
        }

        let freq = if fbase == 0 { fmax } else { fbase };
        let index = match cpu.core_types.iter().position(|&(_, f)| f == freq) {
            Some(index) => index,
            None => {
                cpu.core_types.push((0, freq));
                cpu.core_types.len() - 1
            },
        };
        if let Some(affected) = util::read_trimmed_in(&policy, "affected_cpus") {
            cpu.core_types[index].0 +=
                affected.split(' ').filter(|s| !s.is_empty()).count() as u32;
        }
    }
    true
}

/// detectNumaNodes: the nodeN directories under /sys/devices/system/node.
fn detect_numa_nodes(cpu: &mut CpuResult) {
    let Ok(dir) = std::fs::read_dir("/sys/devices/system/node/") else {
        return;
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("node")
            && name["node".len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        {
            cpu.numa_nodes += 1;
        }
    }
}

/// detectCPUX86 + ffDetectCPUImpl.
pub fn detect() -> Result<CpuResult, &'static str> {
    let mut cpu = CpuResult::default();

    // The temperature stays unset: --temp is not exposed by this CLI.

    let Some(cpuinfo) = util::read_file("/proc/cpuinfo").filter(|content| !content.is_empty())
    else {
        return Err("ffReadFileBuffer(\"/proc/cpuinfo\") failed");
    };

    let (name, vendor, cores, mhz) = parse_cpu_info(&cpuinfo);
    cpu.name = name.unwrap_or_default();
    cpu.vendor = vendor.unwrap_or_default();

    (cpu.cores_logical, cpu.cores_online) = processor_counts();
    cpu.packages = package_count(&cpuinfo);
    cpu.cores_physical = cores
        .and_then(|cores| cores.trim().parse::<u16>().ok())
        .unwrap_or(0);
    if cpu.cores_physical > 0 && cpu.packages > 1 {
        cpu.cores_physical *= cpu.packages;
    }

    // Ref https://github.com/fastfetch-cli/fastfetch/issues/1194
    cpu_x86::detect_by_cpuid(&mut cpu);
    if !detect_frequency(&mut cpu) || cpu.frequency_base == 0 {
        cpu.frequency_base = mhz
            .and_then(|mhz| mhz.trim().parse::<u32>().ok())
            .unwrap_or(0);
    }

    detect_numa_nodes(&mut cpu);

    // ffDetectCPU's name cleanup.
    for remove in [
        " CPU", " FPU", " APU", " Processor", " Dual-Core", " Quad-Core", " Six-Core",
        " Eight-Core", " Ten-Core", " 2-Core", " 4-Core", " 6-Core", " 8-Core", " 10-Core",
        " 12-Core", " 14-Core", " 16-Core",
    ] {
        while let Some(index) = cpu.name.find(remove) {
            cpu.name.replace_range(index..index + remove.len(), "");
        }
    }
    for marker in [" w/ Radeon ", " with Radeon "] {
        if let Some(index) = cpu.name.find(marker) {
            cpu.name.truncate(index);
        }
    }
    if let Some(index) = cpu.name.find('@') {
        cpu.name.truncate(index);
    }
    cpu.name = cpu.name.trim_end().to_string();
    while cpu.name.contains("  ") {
        cpu.name = cpu.name.replace("  ", " ");
    }

    cpu_x86::detect_x86_specific(&mut cpu);

    Ok(cpu)
}

/// The module's text value: "name (cores) @ X GHz".
fn format_value(cpu: &CpuResult) -> String {
    let freq = if cpu.frequency_max > 0 {
        let ghz = cpu.frequency_max as f64 / 1000.0;
        let digits = if ghz >= 100.0 {
            0
        } else if ghz >= 10.0 {
            1
        } else {
            2
        };
        format!(" @ {ghz:.digits$} GHz")
    } else {
        String::new()
    };
    format!("{} ({}){}", cpu.name, cpu.cores_logical, freq)
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(cpu) = detect() else {
        return Vec::new();
    };
    if cpu.vendor.is_empty() && cpu.name.is_empty() && cpu.cores_online <= 1 {
        return Vec::new();
    }
    vec![Row::new("CPU", format_value(&cpu))]
}

/// ffGenerateCPUJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let cpu = match detect() {
        Err(error) => return JsonOutcome::Error(error.to_string()),
        Ok(cpu) => cpu,
    };
    if cpu.vendor.is_empty() && cpu.name.is_empty() && cpu.cores_online <= 1 {
        return JsonOutcome::Error("No CPU detected".to_string());
    }

    let mut obj = Json::obj();
    obj.push("cpu", cpu.name.clone());
    obj.push("vendor", cpu.vendor.clone());
    if cpu.packages == 0 {
        obj.push("packages", Json::Null);
    } else {
        obj.push("packages", cpu.packages as u64);
    }

    let mut cores = Json::obj();
    cores.push("physical", cpu.cores_physical as u64);
    cores.push("logical", cpu.cores_logical as u64);
    cores.push("online", cpu.cores_online as u64);
    obj.push("cores", cores);

    let mut frequency = Json::obj();
    frequency.push("base", cpu.frequency_base as u64);
    frequency.push("max", cpu.frequency_max as u64);
    obj.push("frequency", frequency);

    let mut core_types = Json::Array(Vec::new());
    for &(count, freq) in &cpu.core_types {
        let mut core = Json::obj();
        core.push("count", count as u64);
        core.push("freq", freq as u64);
        core_types.push_value(core);
    }
    obj.push("coreTypes", core_types);

    match cpu.temperature {
        Some(temperature) => obj.push("temperature", temperature),
        None => obj.push("temperature", Json::Null),
    }
    match cpu.march {
        Some(march) => obj.push("march", march),
        None => obj.push("march", Json::Null),
    }
    if cpu.numa_nodes > 0 {
        obj.push("numaNodes", cpu.numa_nodes as u64);
    } else {
        obj.push("numaNodes", Json::Null);
    }
    match cpu.code_name {
        Some(code_name) => obj.push("codeName", code_name),
        None => obj.push("codeName", Json::Null),
    }
    match cpu.technology {
        Some(technology) => obj.push("technology", technology),
        None => obj.push("technology", Json::Null),
    }
    JsonOutcome::Result(obj)
}
