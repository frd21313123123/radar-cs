//! Minimal read-only VPK v2 directory reader.
//!
//! CS2 stores overview metadata and radar textures in `pak01_dir.vpk` plus
//! numbered `pak01_XXX.vpk` archives. Reading the directory tree is enough to
//! locate a file; the payload is read from the owning archive on demand.
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const VPK_SIGNATURE: u32 = 0x55AA_1234;
const VPK_DIRECTORY_ARCHIVE: u16 = 0x7FFF;
const MAX_TREE_BYTES: usize = 64 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Entry {
    pub archive: u16,
    pub offset: u32,
    pub length: u32,
    pub preload: u16,
}

pub struct VpkDir {
    path: PathBuf,
    data_start: u64,
    entries: HashMap<String, Entry>,
}

fn read_cstr(tree: &[u8], pos: &mut usize) -> Result<String, String> {
    let rest = tree.get(*pos..).ok_or("VPK tree ended unexpectedly")?;
    let end = rest.iter().position(|b| *b == 0).ok_or("VPK string is not terminated")?;
    let value = String::from_utf8_lossy(&rest[..end]).into_owned();
    *pos += end + 1;
    Ok(value)
}

impl VpkDir {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
        let mut header = [0u8; 28];
        file.read_exact(&mut header[..12]).map_err(|e| format!("VPK header: {e}"))?;
        let (signature, version, tree_size) = (
            u32::from_le_bytes(header[0..4].try_into().unwrap()),
            u32::from_le_bytes(header[4..8].try_into().unwrap()),
            u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize,
        );
        if signature != VPK_SIGNATURE {
            return Err("Not a VPK directory".into());
        }
        if tree_size == 0 || tree_size > MAX_TREE_BYTES {
            return Err("VPK tree size is invalid".into());
        }
        let (archive_md5_size, other_md5_size, signature_size, header_size) = if version >= 2 {
            file.read_exact(&mut header[12..28]).map_err(|e| format!("VPK header: {e}"))?;
            (
                u32::from_le_bytes(header[12..16].try_into().unwrap()) as u64,
                u32::from_le_bytes(header[16..20].try_into().unwrap()) as u64,
                u32::from_le_bytes(header[20..24].try_into().unwrap()) as u64,
                28u64,
            )
        } else if version == 1 {
            (0, 0, 0, 12u64)
        } else {
            return Err(format!("Unsupported VPK version {version}"));
        };

        let mut tree = vec![0u8; tree_size];
        file.read_exact(&mut tree).map_err(|e| format!("VPK tree: {e}"))?;
        let data_start = header_size
            .checked_add(tree_size as u64)
            .and_then(|v| v.checked_add(archive_md5_size))
            .and_then(|v| v.checked_add(other_md5_size))
            .and_then(|v| v.checked_add(signature_size))
            .ok_or("VPK data offset overflow")?;

        let mut entries = HashMap::new();
        let mut pos = 0usize;
        loop {
            let extension = read_cstr(&tree, &mut pos)?;
            if extension.is_empty() {
                break;
            }
            loop {
                let directory = read_cstr(&tree, &mut pos)?;
                if directory.is_empty() {
                    break;
                }
                loop {
                    let name = read_cstr(&tree, &mut pos)?;
                    if name.is_empty() {
                        break;
                    }
                    if pos + 18 > tree.len() {
                        return Err("VPK entry header is truncated".into());
                    }
                    let crc = u32::from_le_bytes(tree[pos..pos + 4].try_into().unwrap());
                    let preload = u16::from_le_bytes(tree[pos + 4..pos + 6].try_into().unwrap());
                    let archive = u16::from_le_bytes(tree[pos + 6..pos + 8].try_into().unwrap());
                    let offset = u32::from_le_bytes(tree[pos + 8..pos + 12].try_into().unwrap());
                    let length = u32::from_le_bytes(tree[pos + 12..pos + 16].try_into().unwrap());
                    let terminator = u16::from_le_bytes(tree[pos + 16..pos + 18].try_into().unwrap());
                    pos += 18;
                    if terminator != 0xFFFF {
                        return Err("VPK entry terminator is invalid".into());
                    }
                    if preload as usize > tree.len().saturating_sub(pos) {
                        return Err("VPK preload data is truncated".into());
                    }
                    // Preload bytes are rarely used by CS2. They follow the
                    // terminator in the tree and are skipped here.
                    pos += preload as usize;
                    let full = if directory.is_empty() {
                        format!("{name}.{extension}")
                    } else {
                        format!("{directory}/{name}.{extension}")
                    };
                    let _ = crc;
                    entries.insert(full, Entry { archive, offset, length, preload });
                }
            }
        }
        Ok(Self { path: path.to_owned(), data_start, entries })
    }

    pub fn contains(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }

    pub fn entry(&self, path: &str) -> Option<&Entry> {
        self.entries.get(path)
    }

    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }

    pub fn extract(&self, path: &str) -> Result<Vec<u8>, String> {
        let entry = self.entry(path).ok_or_else(|| format!("VPK entry not found: {path}"))?;
        if entry.preload != 0 {
            return Err(format!("VPK preload entries are not supported: {path}"));
        }
        if entry.length as u64 > MAX_FILE_BYTES {
            return Err(format!("VPK entry is too large: {path}"));
        }
        if entry.archive == VPK_DIRECTORY_ARCHIVE {
            let mut file = File::open(&self.path).map_err(|e| format!("VPK open: {e}"))?;
            file.seek(SeekFrom::Start(self.data_start + entry.offset as u64))
                .map_err(|e| format!("VPK seek: {e}"))?;
            let mut buffer = vec![0u8; entry.length as usize];
            file.read_exact(&mut buffer).map_err(|e| format!("VPK read: {e}"))?;
            return Ok(buffer);
        }
        let parent = self.path.parent().ok_or("VPK directory has no parent")?;
        let archive_path = parent.join(format!("pak01_{:03}.vpk", entry.archive));
        let mut file = File::open(&archive_path)
            .map_err(|e| format!("VPK archive {}: {e}", archive_path.display()))?;
        file.seek(SeekFrom::Start(entry.offset as u64))
            .map_err(|e| format!("VPK seek: {e}"))?;
        let mut buffer = vec![0u8; entry.length as usize];
        file.read_exact(&mut buffer).map_err(|e| format!("VPK read: {e}"))?;
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_null_terminated() {
        let mut pos = 0;
        assert_eq!(read_cstr(b"abc\0def\0", &mut pos).unwrap(), "abc");
        assert_eq!(read_cstr(b"abc\0def\0", &mut pos).unwrap(), "def");
    }
}
