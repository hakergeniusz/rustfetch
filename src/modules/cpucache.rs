//! `cpu cache` module: cache hierarchy from /sys/devices/system/cpu/cpuN/
//! cache/indexM, ported from cpucache_linux.c. A cache instance is
//! identified by its shared_cpu_list (+level, size, line size, type); the
//! second cpu reporting the same instance is skipped, and the survivors are
//! grouped by size and type: "6x 48 KiB (D), ...".

use std::collections::HashSet;
use std::path::Path;

use crate::{instance::Instance, modules::Row, util};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CacheType {
    Data,
    Instruction,
    Unified,
    Trace,
}

struct CacheEntry {
    type_: CacheType,
    size: u64,
    num: u32,
}

/// sysfs writes sizes like "48K"; strtoul stops at the suffix.
fn parse_leading_u64(text: &str) -> Option<u64> {
    let end = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    text[..end].parse().ok()
}

/// ffCPUCacheAddItem: equal size+type caches accumulate into one entry.
fn add_item(caches: &mut Vec<CacheEntry>, type_: CacheType, size: u64) {
    if let Some(entry) = caches.iter_mut().find(|entry| entry.type_ == type_ && entry.size == size)
    {
        entry.num += 1;
    } else {
        caches.push(CacheEntry {
            type_,
            size,
            num: 1,
        });
    }
}

fn parse_index(path: &Path) -> Option<(u32, u64, CacheType, String)> {
    let level = parse_leading_u64(&util::read_trimmed_in(path, "level")?)? as u32;
    if !(1..=4).contains(&level) {
        return None;
    }
    let size_kb = parse_leading_u64(&util::read_trimmed_in(path, "size")?)?;
    if size_kb == 0 {
        return None;
    }
    let type_ = match util::read_trimmed_in(path, "type")?.chars().next()? {
        'I' => CacheType::Instruction,
        'D' => CacheType::Data,
        'U' => CacheType::Unified,
        'T' => CacheType::Trace,
        _ => return None,
    };
    let shared_list = util::read_trimmed_in(path, "shared_cpu_list").unwrap_or_default();
    Some((level, size_kb * 1024, type_, shared_list))
}

fn detect() -> Vec<Vec<CacheEntry>> {
    // One grouping per level L1..L4; the dedup set spans all of them.
    let mut result: [Vec<CacheEntry>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut seen = HashSet::new();
    let Ok(cpus) = std::fs::read_dir("/sys/devices/system/cpu") else {
        return Vec::new();
    };
    for cpu in cpus.flatten() {
        let cpu_name = cpu.file_name().to_string_lossy().into_owned();
        if !cpu_name.starts_with("cpu")
            || cpu_name[3..].chars().next().is_none_or(|c| !c.is_ascii_digit())
        {
            continue;
        }
        let Ok(indexes) = std::fs::read_dir(cpu.path().join("cache")) else {
            continue;
        };
        for index in indexes.flatten() {
            let index_name = index.file_name().to_string_lossy().into_owned();
            if !index_name.starts_with("index")
                || index_name[5..].chars().next().is_none_or(|c| !c.is_ascii_digit())
            {
                continue;
            }
            if let Some((level, size, type_, shared_list)) = parse_index(&index.path()) {
                // "[list]_level_size_type" — the shared list already pins
                // down one physical instance.
                let identity = format!("[{shared_list}]_{level}_{size}_{type_:?}");
                if seen.insert(identity) {
                    add_item(&mut result[(level - 1) as usize], type_, size);
                }
            }
        }
    }
    result.into()
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let levels = detect();

    levels
        .iter()
        .enumerate()
        .filter(|(_, caches)| !caches.is_empty())
        .map(|(level, caches)| {
            let mut value = String::new();
            for cache in caches {
                if !value.is_empty() {
                    value.push_str(", ");
                }
                if cache.num > 1 {
                    value.push_str(&format!("{}x", cache.num));
                }
                value.push_str(&util::fmt_bytes(cache.size));
                let letter = match cache.type_ {
                    CacheType::Data => 'D',
                    CacheType::Instruction => 'I',
                    CacheType::Unified => 'U',
                    CacheType::Trace => 'T',
                };
                value.push_str(&format!(" ({letter})"));
            }
            Row::new(&format!("CPU Cache (L{})", level + 1), value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn groups_caches_by_size_and_type() {
        let mut caches = Vec::new();
        // Six distinct cache instances of the same size and type collapse
        // into "6x".
        super::add_item(&mut caches, super::CacheType::Data, 48 * 1024);
        super::add_item(&mut caches, super::CacheType::Data, 48 * 1024);
        super::add_item(&mut caches, super::CacheType::Data, 48 * 1024);
        super::add_item(&mut caches, super::CacheType::Data, 48 * 1024);
        super::add_item(&mut caches, super::CacheType::Data, 48 * 1024);
        super::add_item(&mut caches, super::CacheType::Data, 48 * 1024);
        super::add_item(&mut caches, super::CacheType::Instruction, 32 * 1024);
        assert_eq!(caches.len(), 2);
        assert_eq!(caches[0].num, 6);
        assert_eq!(caches[1].num, 1);
    }
}
