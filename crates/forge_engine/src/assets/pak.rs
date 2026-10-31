//! Minimal read-only archive used to ship scenes, scripts and the manifest as a
//! single file. Layout (little endian):
//!
//! ```text
//! "FORGEPAK" | version:u32 | count:u32
//! count x { path_len:u16 | path | offset:u64 | len:u64 }   (offset is relative to data start)
//! data...
//! ```

use std::{collections::HashMap, fs, path::Path};

use anyhow::{ensure, Context, Result};

const MAGIC: &[u8; 8] = b"FORGEPAK";
const VERSION: u32 = 1;
/// Smallest possible directory entry: `path_len:u16 | offset:u64 | len:u64` with an empty path.
const ENTRY_MIN: usize = 2 + 8 + 8;

fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_owned()
}

#[derive(Default)]
pub struct PakBuilder {
    entries: Vec<(String, Vec<u8>)>,
}

impl PakBuilder {
    pub fn add(&mut self, path: impl AsRef<str>, data: Vec<u8>) {
        self.entries.push((normalize(path.as_ref()), data));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Writes the archive and returns its size in bytes.
    pub fn write(&self, out: &Path) -> Result<u64> {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&VERSION.to_le_bytes());
        buf.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        let mut offset = 0u64;
        for (path, data) in &self.entries {
            ensure!(path.len() <= u16::MAX as usize, "path too long: {path}");
            buf.extend_from_slice(&(path.len() as u16).to_le_bytes());
            buf.extend_from_slice(path.as_bytes());
            buf.extend_from_slice(&offset.to_le_bytes());
            buf.extend_from_slice(&(data.len() as u64).to_le_bytes());
            offset += data.len() as u64;
        }
        for (_, data) in &self.entries {
            buf.extend_from_slice(data);
        }
        fs::write(out, &buf).with_context(|| format!("writing {}", out.display()))?;
        Ok(buf.len() as u64)
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).context("pak truncated")?;
        ensure!(end <= self.data.len(), "pak truncated");
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }
}

pub struct Pak {
    data: Vec<u8>,
    index: HashMap<String, (usize, usize)>,
}

impl Pak {
    pub fn open(path: &Path) -> Result<Self> {
        let data = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_bytes(data)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        let mut r = Reader { data: &data, pos: 0 };
        ensure!(r.take(8)? == MAGIC, "not a Forge pak file");
        let version = r.u32()?;
        ensure!(version == VERSION, "unsupported pak version {version}");
        let count = r.u32()? as usize;
        // Every entry takes at least path_len(2) + offset(8) + len(8) bytes, so a count that
        // cannot fit in the file is corrupt; never preallocate from an untrusted number.
        ensure!(count <= (data.len() - r.pos) / ENTRY_MIN, "pak entry count {count} does not fit in the file");
        let mut raw = Vec::with_capacity(count);
        for _ in 0..count {
            let len = r.u16()? as usize;
            let path = std::str::from_utf8(r.take(len)?)?.to_owned();
            let offset = r.u64()? as usize;
            let size = r.u64()? as usize;
            raw.push((path, offset, size));
        }
        let base = r.pos;
        let mut index = HashMap::with_capacity(count);
        for (path, offset, size) in raw {
            let start = base.checked_add(offset).context("pak offset overflow")?;
            let end = start.checked_add(size).context("pak size overflow")?;
            ensure!(end <= data.len(), "pak entry '{path}' is out of bounds");
            index.insert(path, (start, end));
        }
        Ok(Self { data, index })
    }

    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.index.get(path).map(|&(s, e)| &self.data[s..e])
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.index.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("forge_pak_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.pak");

        let mut b = PakBuilder::default();
        b.add("scenes/a.json", b"{}".to_vec());
        b.add("scripts\\b.luau", b"return {}".to_vec());
        b.write(&path).unwrap();

        let pak = Pak::open(&path).unwrap();
        assert_eq!(pak.get("scenes/a.json"), Some(&b"{}"[..]));
        assert_eq!(pak.get("scripts/b.luau"), Some(&b"return {}"[..]));
        assert!(pak.get("missing").is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn huge_entry_count_is_rejected_without_allocating() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(Pak::from_bytes(bytes).is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(Pak::from_bytes(b"not a pak".to_vec()).is_err());
        assert!(Pak::from_bytes(Vec::new()).is_err());
    }
}
