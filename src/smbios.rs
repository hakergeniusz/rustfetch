//! SMBIOS table loading and structure walking, ported from
//! common/impl/smbios.c. /sys/firmware/dmi/tables/DMI is tried first; the
//! EFI systab and BIOS-memory fallbacks read /dev/mem (pread, then mmap),
//! which needs privileges. Structures are addressed by offset into the raw
//! table; a 4-byte header {type, length, handle} is followed by the
//! formatted section and the NUL-terminated string area.

pub(crate) const END_OF_TABLE: u8 = 127;

pub(crate) struct SmbiosTable {
    data: Vec<u8>,
    /// Offset of the first structure of each type; None when absent.
    first: [Option<usize>; 128],
    /// Offset of the last structure, upstream's end-of-table pointer.
    end: usize,
}

impl SmbiosTable {
    /// ffGetSmbiosHeaderTable: platform sources, then the BIOS scan.
    pub(crate) fn load() -> Option<Self> {
        if let Some(table) = Self::from_platform() {
            return Some(table);
        }
        Self::from_fallback().and_then(Self::parse)
    }

    /// fillTableBufferPlatform on Linux: the kernel-exposed table file,
    /// else the entry point address from /sys/firmware/efi/systab.
    fn from_platform() -> Option<Self> {
        if let Ok(data) = std::fs::read("/sys/firmware/dmi/tables/DMI")
            && !data.is_empty()
        {
            return Self::parse(data);
        }

        let systab = std::fs::read_to_string("/sys/firmware/efi/systab").ok()?;
        let entry = ["SMBIOS3=", "SMBIOS="]
            .iter()
            .find_map(|key| parse_prop_line(&systab, key))?;
        let address = u64::from_str_radix(entry.trim_start_matches("0x"), 16).ok()?;
        if address == 0 {
            return None;
        }
        read_entry_point_and_table(address)
    }

    /// fillTableBufferFallback: scan the legacy BIOS region for an entry
    /// point (works on legacy BIOS only, not on UEFI systems).
    fn from_fallback() -> Option<Vec<u8>> {
        let region = read_physical_memory_at(0xF0000, 0x10000)?;
        for offset in (0..=0xffe0usize).step_by(0x10) {
            let window = &region[offset..];
            if window.len() < 0x1F {
                break;
            }
            if window.starts_with(b"_SM3_") {
                if window[6] != 0x18 {
                    return None;
                }
                let length = u32_le(window, 0x0C);
                let address = u64_le(window, 0x10);
                return read_physical_memory_at(address, length);
            } else if window.starts_with(b"_SM_") {
                if window[6] != 0x1F {
                    return None;
                }
                let length = u32::from(u16_le(window, 0x16));
                let address = u32_le(window, 0x18) as u64;
                return read_physical_memory_at(address, length);
            }
        }
        None
    }

    /// parseSmbiosTable: first-occurrence table keyed by type, ending at the
    /// last structure (the end-of-table marker when well-formed).
    fn parse(data: Vec<u8>) -> Option<Self> {
        let mut first = [None; 128];
        let mut end = None;
        let mut offset = 0usize;
        while offset + 4 < data.len() {
            let type_ = data[offset];
            let length = data[offset + 1] as usize;
            end = Some(offset);
            if length < 4 {
                break;
            }
            if u16_le(&data, offset + 2) >= 0xFF00 {
                break;
            }
            if offset + length > data.len() {
                break;
            }
            if type_ < END_OF_TABLE {
                if first[type_ as usize].is_none() {
                    first[type_ as usize] = Some(offset);
                }
            } else if type_ == END_OF_TABLE {
                break;
            }
            offset = next_entry(&data, offset, length);
        }
        let end = end?;
        first[END_OF_TABLE as usize] = Some(end);
        Some(Self { data, first, end })
    }

    /// The first structure offset registered for `type_`.
    pub(crate) fn first_offset(&self, type_: u8) -> Option<usize> {
        self.first[type_ as usize]
    }

    /// Offsets of every structure of `type_`, from its first occurrence up
    /// to the end-of-table pointer, in table order.
    pub(crate) fn walk(&self, type_: u8) -> Vec<usize> {
        let Some(mut offset) = self.first[type_ as usize] else {
            return Vec::new();
        };
        let mut offsets = Vec::new();
        while offset != self.end {
            if self.data[offset] == type_ {
                offsets.push(offset);
            }
            offset = next_entry(&self.data, offset, self.data[offset + 1] as usize);
        }
        offsets
    }

    /// Header length (the formatted section size, including the 4 header
    /// bytes) of the structure at `offset`.
    pub(crate) fn header_length(&self, offset: usize) -> usize {
        self.data[offset + 1] as usize
    }

    /// ffSmbiosLocateString: the `index`-th (1-based) string in the string
    /// area beginning at `start` (an offset into the raw table).
    pub(crate) fn locate_string(&self, start: usize, index: u8) -> Option<&str> {
        if index == 0 || self.data.get(start).copied().unwrap_or(0) == 0 {
            return None;
        }
        let mut p = start;
        let mut n = index;
        while n > 1 {
            n -= 1;
            while p < self.data.len() && self.data[p] != 0 {
                p += 1;
            }
            p += 1;
        }
        let len = self.data[p..].iter().position(|&b| b == 0)?;
        std::str::from_utf8(&self.data[p..p + len]).ok()
    }

    pub(crate) fn u8_at(&self, offset: usize, field: usize) -> u8 {
        self.data.get(offset + field).copied().unwrap_or(0)
    }

    pub(crate) fn u16_at(&self, offset: usize, field: usize) -> u16 {
        u16_le(&self.data, offset + field)
    }

    pub(crate) fn u32_at(&self, offset: usize, field: usize) -> u32 {
        u32_le(&self.data, offset + field)
    }
}

/// ffSmbiosNextEntry: skip the formatted section and the string area, whose
/// terminator is a double NUL even with no strings.
fn next_entry(data: &[u8], offset: usize, length: usize) -> usize {
    let mut p = offset + length;
    if data.get(p).copied().unwrap_or(0) != 0 {
        loop {
            while p < data.len() && data[p] != 0 {
                p += 1;
            }
            p += 1; // strlen(p) + 1
            if p >= data.len() || data[p] == 0 {
                break;
            }
        }
    } else {
        p += 1;
    }
    p + 1
}

fn u16_le(data: &[u8], at: usize) -> u16 {
    data.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .unwrap_or(0)
}

fn u32_le(data: &[u8], at: usize) -> u32 {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .unwrap_or(0)
}

fn u64_le(data: &[u8], at: usize) -> u64 {
    data.get(at..at + 8)
        .map(|b| {
            u64::from_le_bytes([
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
            ])
        })
        .unwrap_or(0)
}

/// ffParsePropLines for one key: the first "KEY=value" line's value.
fn parse_prop_line(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let value = line.strip_prefix(key)?;
        Some(value.trim_start().to_string())
    })
}

/// readPhysicalMemory: pread, falling back to a page-aligned mmap copy.
fn read_physical_memory_at(address: u64, length: u32) -> Option<Vec<u8>> {
    use std::os::unix::io::AsRawFd;
    let file = std::fs::File::open("/dev/mem").ok()?;
    let fd = file.as_raw_fd();

    let mut buffer = vec![0u8; length as usize];
    // SAFETY: pread on an owned fd into our own buffer of matching length.
    let read = unsafe {
        libc::pread(fd, buffer.as_mut_ptr() as *mut libc::c_void, length as usize, address as libc::off_t)
    };
    if read == length as isize {
        return Some(buffer);
    }

    let page_mask = !(unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64 - 1);
    let aligned = address & page_mask;
    let page_offset = (address - aligned) as usize;
    let map_length = page_offset + length as usize;
    // SAFETY: MAP_SHARED read-only mapping of /dev/mem, unmapped below.
    let map = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            map_length,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            aligned as libc::off_t,
        )
    };
    if map == libc::MAP_FAILED {
        return None;
    }
    // SAFETY: map_length bytes are readable from the successful mapping.
    let mapped = unsafe { std::slice::from_raw_parts(map as *const u8, map_length) };
    buffer.copy_from_slice(&mapped[page_offset..page_offset + length as usize]);
    // SAFETY: releases the mapping made above.
    unsafe { libc::munmap(map, map_length) };
    Some(buffer)
}

/// The entry point at a physical address (2.0 = 0x1F bytes, 3.0 = 0x18),
/// then the structure table it points at.
fn read_entry_point_and_table(address: u64) -> Option<SmbiosTable> {
    let entry = read_physical_memory_at(address, 0x1F)?;
    if entry.starts_with(b"_SM_") {
        if entry[6] != 0x1F {
            return None;
        }
        let length = u32::from(u16_le(&entry, 0x16));
        let table_address = u32_le(&entry, 0x18) as u64;
        return SmbiosTable::parse(read_physical_memory_at(table_address, length)?);
    }
    if entry.starts_with(b"_SM3_") {
        if entry[6] != 0x18 {
            return None;
        }
        let length = u32_le(&entry, 0x0C);
        let table_address = u64_le(&entry, 0x10);
        return SmbiosTable::parse(read_physical_memory_at(table_address, length)?);
    }
    None
}

#[cfg(test)]
mod tests {
    /// A minimal well-formed table: type 4 (header 4 bytes, one string "X"),
    /// then type 127 (no strings).
    #[test]
    fn walks_strings_and_entries() {
        let data = [
            4u8, 4, 0x01, 0x00, b'X', 0, 0, // type 4, strings "X", terminator
            127, 4, 0x02, 0x00, 0, 0, // type 127, no strings
        ];
        let Some(table) = super::SmbiosTable::parse(data.to_vec()) else {
            panic!("well-formed table must parse");
        };
        assert_eq!(table.first_offset(4), Some(0));
        assert_eq!(table.first_offset(127), Some(7));
        assert_eq!(table.walk(4), vec![0]);
        assert_eq!(table.header_length(0), 4);
        // locate_string is 1-based over the string area; an index past the
        // last string lands on the terminator and yields "" (upstream
        // returns the pointer without checking).
        assert_eq!(table.locate_string(4, 1), Some("X"));
        assert_eq!(table.locate_string(4, 2), Some(""));
        assert_eq!(table.locate_string(4, 0), None);
    }

    /// Two strings "A" and "BB": the terminator is the NUL after the last
    /// string plus one more.
    #[test]
    fn skips_multi_string_areas() {
        let data = [
            1u8, 4, 0x01, 0x00, b'A', 0, b'B', b'B', 0, 0, // strings A, BB
            127, 4, 0x02, 0x00, 0, 0,
        ];
        let Some(table) = super::SmbiosTable::parse(data.to_vec()) else {
            panic!("well-formed table must parse");
        };
        assert_eq!(table.first_offset(127), Some(10));
        assert_eq!(table.locate_string(4, 1), Some("A"));
        assert_eq!(table.locate_string(4, 2), Some("BB"));
    }

    /// A structure with no strings still ends after the double NUL.
    #[test]
    fn handles_empty_string_areas() {
        let data = [
            3u8, 9, 0x01, 0x00, // 9-byte formatted section, no strings
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE,
            0, 0, // double-NUL terminator
            127, 4, 0x02, 0x00, 0, 0,
        ];
        let Some(table) = super::SmbiosTable::parse(data.to_vec()) else {
            panic!("well-formed table must parse");
        };
        assert_eq!(table.first_offset(127), Some(11));
    }

    #[test]
    fn rejects_broken_tables() {
        // handle >= 0xFF00 stops the walk before the type is registered,
        // but the end pointer is still recorded, like upstream's
        // endOfTable-at-top-of-loop behavior.
        let table = super::SmbiosTable::parse(vec![4u8, 4, 0xFF, 0xFF, 0, 0]);
        assert!(table.is_some());
        assert_eq!(table.and_then(|t| t.first_offset(4)), None);
        // no structures at all
        assert!(super::SmbiosTable::parse(Vec::new()).is_none());
    }
}
