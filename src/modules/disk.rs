//! `disk` module: mounted filesystems from /proc/mounts with statvfs
//! usage, names from /dev/disk/by-label, types (regular/subvolume/hidden/
//! external/read-only) and statx birth times, ported from
//! detection/disk/disk_linux.c. Text skips types outside the default
//! showTypes (regular/external/read-only); JSON reports every entry.

use std::os::unix::fs::MetadataExt;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::{colorize_percent, Row},
    util,
};

// FF_DISK_VOLUME_TYPE bits, in the JSON append order.
const VOLUME_REGULAR: u8 = 1 << 0;
const VOLUME_EXTERNAL: u8 = 1 << 1;
const VOLUME_SUBVOLUME: u8 = 1 << 2;
const VOLUME_HIDDEN: u8 = 1 << 3;
const VOLUME_READONLY: u8 = 1 << 4;
const VOLUME_UNKNOWN: u8 = 1 << 5;

pub struct DiskResult {
    pub mount_from: String,
    pub mountpoint: String,
    pub filesystem: String,
    pub name: String,
    pub bytes_total: u64,
    pub bytes_free: u64,
    pub files_total: u32,
    pub files_used: u32,
    /// Birth time in ms, 0 when unavailable.
    pub create_time: u64,
    pub volume_type: u8,
}

/// One /proc/mounts entry, with the octal escapes getmntent decodes.
struct MountEntry {
    fsname: String,
    dir: String,
    fstype: String,
    opts: String,
}

fn parse_mounts(content: &str) -> Vec<MountEntry> {
    content
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some(MountEntry {
                fsname: util::decode_octal_escapes(fields.next()?),
                dir: util::decode_octal_escapes(fields.next()?),
                fstype: fields.next()?.to_string(),
                opts: fields.next()?.to_string(),
            })
        })
        .collect()
}

/// hasmntopt: the option appears as a full comma-separated token, which
/// may carry a value ("ro=").
fn has_opt(opts: &str, option: &str) -> bool {
    opts.split(',')
        .any(|opt| opt == option || opt.strip_prefix(option).is_some_and(|rest| rest.starts_with('=')))
}

/// isPhysicalDevice: real block devices, "/" always, plus the zfs/sshfs/
/// bcachefs/drvfs specials and the loop/ram/fd exclusions.
fn is_physical_device(entry: &MountEntry) -> bool {
    if entry.dir == "/" {
        return true;
    }
    if entry.fsname == "none" {
        return false;
    }
    if entry.fstype == "9p" {
        return entry.opts.contains("aname=drvfs");
    }
    if entry.fstype == "zfs" || entry.fstype == "fuse.sshfs" || entry.fstype == "bcachefs" {
        return true;
    }
    if !entry.fsname.starts_with("/dev/") {
        return false;
    }
    let tail = &entry.fsname["/dev/".len()..];
    if tail.starts_with("loop") || tail.starts_with("ram") || tail.starts_with("fd") {
        return false;
    }
    if entry.dir.starts_with("/bedrock/") {
        return false;
    }
    // Only block devices count.
    let Ok(c_path) = std::ffi::CString::new(entry.fsname.as_str()) else {
        return false;
    };
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: c_path is a valid NUL-terminated path; stat only writes into stat.
    if unsafe { libc::stat(c_path.as_ptr(), &mut stat) } != 0 {
        return false;
    }
    (stat.st_mode & libc::S_IFMT) == libc::S_IFBLK
}

/// detectNameFromPath: the directory entry whose inode equals the device's.
fn detect_name_from_path(device_ino: u64, base: &std::path::Path) -> Option<String> {
    for entry in std::fs::read_dir(base).ok()?.flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(entry_stat) = entry.metadata() else {
            continue;
        };
        if entry_stat.ino() == device_ino {
            return Some(entry.file_name().to_string_lossy().into_owned());
        }
    }
    None
}

/// detectName: the device's label symlink name, then its partition label.
fn detect_name(mount_from: &str) -> String {
    let Ok(c_path) = std::ffi::CString::new(mount_from) else {
        return String::new();
    };
    let mut device_stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: c_path is a valid NUL-terminated path; stat only writes into device_stat.
    if unsafe { libc::stat(c_path.as_ptr(), &mut device_stat) } != 0 {
        return String::new();
    }
    let ino = device_stat.st_ino;

    let mut name = detect_name_from_path(ino, std::path::Path::new("/dev/disk/by-label/"))
        .or_else(|| detect_name_from_path(ino, std::path::Path::new("/dev/disk/by-partlabel/")))
        .unwrap_or_default();
    if !name.is_empty() {
        name = util::decode_octal_escapes(&name);
    }
    name
}

/// isSubvolume: btrfs subvolumes mount the same device twice; zfs datasets
/// share their pool prefix.
fn is_subvolume(disks: &[DiskResult], current: &DiskResult) -> bool {
    if current.mount_from == "drvfs" {
        return false;
    }
    if current.filesystem == "zfs" {
        let Some(index) = current.mount_from.find('/') else {
            return false;
        };
        let zpool = &current.mount_from[..index];
        return disks
            .iter()
            .any(|other| other.filesystem == "zfs" && other.mount_from.starts_with(zpool));
    }
    disks
        .iter()
        .any(|other| other.mount_from == current.mount_from)
}

/// isRemovable: the sysfs block device's "removable" file.
fn is_removable(mount_from: &str) -> bool {
    if !mount_from.starts_with("/dev/") {
        return false;
    }
    let partition = format!("/sys/class/block/{}", &mount_from["/dev/".len()..]);
    let Ok(volume) = std::fs::canonicalize(&partition) else {
        return false;
    };
    let Some(parent) = volume.parent() else {
        return false;
    };
    util::read_trimmed_in(parent, "removable").as_deref() == Some("1")
}

/// detectType.
fn detect_type(disks: &[DiskResult], current: &mut DiskResult, entry: &MountEntry) {
    current.volume_type = if has_opt(&entry.opts, "x-gvfs-hide") || has_opt(&entry.opts, "hidden")
    {
        VOLUME_HIDDEN
    } else if is_subvolume(disks, current) {
        VOLUME_SUBVOLUME
    } else if is_removable(&current.mount_from) {
        VOLUME_EXTERNAL
    } else {
        VOLUME_REGULAR
    };
    if has_opt(&entry.opts, "ro") {
        current.volume_type |= VOLUME_READONLY;
    }
}

/// detectStats: statvfs usage and the statx birth time.
fn detect_stats(disk: &mut DiskResult) {
    let Ok(c_path) = std::ffi::CString::new(disk.mountpoint.as_str()) else {
        return;
    };
    let mut fs: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: c_path is valid for the call; statvfs only writes into fs.
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut fs) } != 0 {
        return; // zeroed values
    }

    let frsize = fs.f_frsize as u64;
    disk.bytes_total = fs.f_blocks as u64 * frsize;
    // calcType defaults to FREE.
    disk.bytes_free = fs.f_bfree as u64 * frsize;

    if fs.f_files >= fs.f_ffree {
        disk.files_total = fs.f_files as u32;
        disk.files_used = disk.files_total - fs.f_ffree as u32;
    }

    // SYS_statx with STATX_BTIME; birth times before Linux existed are noise.
    //
    // The buffer must match the kernel UAPI struct exactly: the syscall has
    // no buffer-size argument and always copies the full 256-byte struct,
    // so a short buffer is a stack overflow (absorbed by debug builds'
    // larger frames, fatal in release).
    #[repr(C)]
    struct StatxTimestamp {
        sec: i64,
        nsec: u32,
        reserved: i32,
    }
    #[repr(C)]
    struct Statx {
        mask: u32,
        blksize: u32,
        attributes: u64,
        nlink: u32,
        uid: u32,
        gid: u32,
        mode: u16,
        spare0: u16,
        ino: u64,
        size: u64,
        blocks: u64,
        attributes_mask: u64,
        atime: StatxTimestamp,
        btime: StatxTimestamp,
        ctime: StatxTimestamp,
        mtime: StatxTimestamp,
        rdev_major: u32,
        rdev_minor: u32,
        dev_major: u32,
        dev_minor: u32,
        mnt_id: u64,
        // Everything newer kernels fill (dio read/write alignments, memcg
        // id, future spares) lands in this tail.
        spare: [u8; 104],
    }
    const _: () = assert!(std::mem::size_of::<Statx>() == 256);
    let mut stx: Statx = unsafe { std::mem::zeroed() };
    // SAFETY: the syscall only writes into stx; c_path stays valid.
    let ret = unsafe {
        libc::syscall(libc::SYS_statx, 0, c_path.as_ptr(), 0, 0x800u32, &mut stx as *mut Statx)
    };
    if ret == 0 && (stx.mask & 0x800) != 0 && stx.btime.sec > 685065600 {
        disk.create_time = (stx.btime.sec as u64 * 1000) + (stx.btime.nsec as u64 / 1_000_000);
    }
}

/// ffDetectDisks.
pub fn detect() -> Result<Vec<DiskResult>, &'static str> {
    let Some(content) = util::read_file("/proc/mounts") else {
        return Err("setmntent(\"/proc/mounts\", \"r\") == nullptr");
    };

    let mut disks: Vec<DiskResult> = Vec::new();
    for entry in parse_mounts(&content) {
        // The default folders list is empty: only physical devices.
        if !is_physical_device(&entry) {
            continue;
        }

        // Default hideFolders: /efi:/boot:/boot/*
        if ["/efi", "/boot", "/boot/*"]
            .iter()
            .any(|pattern| util::glob_match(pattern, &entry.dir))
        {
            continue;
        }

        let mut disk = DiskResult {
            mount_from: entry.fsname.clone(),
            mountpoint: entry.dir.clone(),
            filesystem: entry.fstype.clone(),
            name: String::new(),
            bytes_total: 0,
            bytes_free: 0,
            files_total: 0,
            files_used: 0,
            create_time: 0,
            volume_type: 0,
        };
        disk.name = detect_name(&disk.mount_from);
        detect_type(&disks, &mut disk, &entry);
        detect_stats(&mut disk);
        disks.push(disk);
    }

    // ffDetectDisks: sort by mountpoint, then flag zero-total volumes as
    // unknown (which also hides them from the default text output).
    disks.sort_by(|a, b| a.mountpoint.cmp(&b.mountpoint));
    for disk in &mut disks {
        if disk.bytes_total == 0 {
            disk.volume_type |= VOLUME_UNKNOWN;
        }
    }
    Ok(disks)
}

/// The type names inside the text bracket, in the printer's order.
fn type_names(volume_type: u8) -> Vec<&'static str> {
    let mut names = Vec::new();
    if volume_type & VOLUME_EXTERNAL != 0 {
        names.push("External");
    }
    if volume_type & VOLUME_SUBVOLUME != 0 {
        names.push("Subvolume");
    }
    if volume_type & VOLUME_HIDDEN != 0 {
        names.push("Hidden");
    }
    if volume_type & VOLUME_READONLY != 0 {
        names.push("Read-only");
    }
    names
}

/// The default showTypes: regular, external and read-only volumes pass.
fn show_types_match(volume_type: u8) -> bool {
    volume_type & !(VOLUME_REGULAR | VOLUME_EXTERNAL | VOLUME_READONLY) == 0
}

/// printDisk's default format for one disk.
fn item_value(disk: &DiskResult, color: bool, df: &crate::format::DisplayFormat) -> String {
    let bytes_used = disk.bytes_total.saturating_sub(disk.bytes_free);
    let pct = if disk.bytes_total > 0 {
        bytes_used as f64 * 100.0 / disk.bytes_total as f64
    } else {
        0.0
    };

    let mut value = if disk.bytes_total > 0 {
        // "used / total " then the percentage then a separator space
        let mut used_pretty = String::new();
        crate::format::size_append_num(&mut used_pretty, bytes_used, df);
        let mut total_pretty = String::new();
        crate::format::size_append_num(&mut total_pretty, disk.bytes_total, df);
        format!("{} / {} ({}) ", used_pretty, total_pretty, colorize_percent(pct, 50, 80, color))
    } else {
        "Unknown ".to_string()
    };

    if !disk.filesystem.is_empty() {
        value.push_str(&format!("- {} ", disk.filesystem));
    }

    let names = type_names(disk.volume_type);
    if !names.is_empty() {
        value.push_str(&format!("[{}]", names.join(", ")));
    }
    value.trim_end().to_string()
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(disks) = detect() else {
        return Vec::new();
    };
    disks
        .iter()
        .filter(|disk| show_types_match(disk.volume_type))
        .map(|disk| {
            Row::new(
                &format!("{} ({})", instance.display_name("disk"), disk.mountpoint),
                item_value(disk, instance.color, &instance.display_format),
            )
        })
        .collect()
}

/// ffGenerateDiskJsonResult: every detected disk, no type filtering.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(disks) => {
            let mut arr = Json::Array(Vec::new());
            for disk in &disks {
                let mut obj = Json::obj();
                let mut bytes = Json::obj();
                bytes.push("free", disk.bytes_free);
                bytes.push("total", disk.bytes_total);
                obj.push("bytes", bytes);

                let mut files = Json::obj();
                if disk.files_total == 0 && disk.files_used == 0 {
                    files.push("total", Json::Null);
                    files.push("used", Json::Null);
                } else {
                    files.push("total", disk.files_total as u64);
                    files.push("used", disk.files_used as u64);
                }
                obj.push("files", files);

                obj.push("filesystem", disk.filesystem.clone());
                obj.push("mountpoint", disk.mountpoint.clone());
                obj.push("mountFrom", disk.mount_from.clone());
                obj.push("name", disk.name.clone());

                let mut types = Json::Array(Vec::new());
                if disk.volume_type & VOLUME_REGULAR != 0 {
                    types.push_value(Json::Str("Regular".to_string()));
                }
                if disk.volume_type & VOLUME_EXTERNAL != 0 {
                    types.push_value(Json::Str("External".to_string()));
                }
                if disk.volume_type & VOLUME_SUBVOLUME != 0 {
                    types.push_value(Json::Str("Subvolume".to_string()));
                }
                if disk.volume_type & VOLUME_HIDDEN != 0 {
                    types.push_value(Json::Str("Hidden".to_string()));
                }
                if disk.volume_type & VOLUME_READONLY != 0 {
                    types.push_value(Json::Str("Read-only".to_string()));
                }
                if disk.volume_type & VOLUME_UNKNOWN != 0 {
                    types.push_value(Json::Str("Unknown".to_string()));
                }
                obj.push("volumeType", types);

                if disk.create_time != 0 {
                    obj.push("createTime", util::time_to_full_str(disk.create_time));
                } else {
                    obj.push("createTime", Json::Null);
                }
                arr.push_value(obj);
            }
            JsonOutcome::Result(arr)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_line_parses_with_octal_escapes() {
        let entries = parse_mounts("/dev/nvme0n1p2 / btrfs rw,ssd 0 0\n/dev/sda1 /mnt/with\\040space ext4 rw 0 0\n");
        assert_eq!(entries[0].fsname, "/dev/nvme0n1p2");
        assert_eq!(entries[0].opts, "rw,ssd");
        assert_eq!(entries[1].dir, "/mnt/with space");
    }

    #[test]
    fn options_match_full_tokens() {
        assert!(has_opt("rw,ro,relatime", "ro"));
        assert!(has_opt("rw,ro=subvol", "ro"));
        assert!(!has_opt("rw,rotate", "ro"));
    }

    #[test]
    fn types_render_in_printer_order() {
        assert_eq!(
            type_names(VOLUME_READONLY | VOLUME_SUBVOLUME),
            vec!["Subvolume", "Read-only"]
        );
        assert!(type_names(VOLUME_REGULAR).is_empty());
    }

    #[test]
    fn glob_matches_default_hide_folders() {
        assert!(util::glob_match("/boot/*", "/boot/efi"));
        assert!(util::glob_match("/efi", "/efi"));
        assert!(!util::glob_match("/boot/*", "/boot"));
    }
}
