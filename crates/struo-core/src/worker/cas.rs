use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use struo_cas::Digest;

pub(super) fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn charge(bytes: u64) -> io::Result<u64> {
    bytes
        .max(1)
        .checked_next_multiple_of(4096)
        .ok_or_else(|| invalid("CAS capacity overflow"))
}

pub(super) struct Cas {
    root: PathBuf,
    pub capacity: u64,
    used: u64,
}

impl Cas {
    pub fn open(root: PathBuf, capacity: u64) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        let mut used = 0_u64;
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let metadata = entry.file_type()?;
            if !metadata.is_file() {
                return Err(invalid("CAS contains non-regular entry"));
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".tmp") {
                fs::remove_file(entry.path())?;
                continue;
            }
            let expected = parse_digest(&name)?;
            if Digest::of_reader(File::open(entry.path())?)? != expected {
                return Err(invalid("CAS contains corrupt object"));
            }
            used = used
                .checked_add(charge(entry.metadata()?.len())?)
                .ok_or_else(|| invalid("CAS size overflow"))?;
        }
        if used > capacity {
            return Err(invalid("CAS capacity exceeded"));
        }
        Ok(Self {
            root,
            capacity,
            used,
        })
    }

    pub fn path(&self, digest: &Digest) -> PathBuf {
        self.root.join(digest.to_hex())
    }

    pub fn available(&self) -> u64 {
        self.capacity - self.used
    }

    // Called only between actions, so no live input or result can be evicted.
    pub fn make_room(&mut self, bytes: u64) -> io::Result<()> {
        self.make_room_pinned(bytes, &HashSet::new())
    }

    pub fn make_room_pinned(&mut self, bytes: u64, pinned: &HashSet<Digest>) -> io::Result<()> {
        let bytes = charge(bytes)?;
        if bytes > self.capacity {
            return Err(invalid("CAS capacity exceeded"));
        }
        for entry in fs::read_dir(&self.root)? {
            if self.available() >= bytes {
                break;
            }
            let entry = entry?;
            let digest = parse_digest(&entry.file_name().to_string_lossy())?;
            if pinned.contains(&digest) {
                continue;
            }
            let size = charge(entry.metadata()?.len())?;
            fs::remove_file(entry.path())?;
            self.used -= size;
        }
        Ok(())
    }

    pub fn publish(&mut self, source: &Path, expected: Option<Digest>) -> io::Result<Digest> {
        let size = charge(source.metadata()?.len())?;
        let digest = Digest::of_reader(File::open(source)?)?;
        if expected.is_some_and(|expected| expected != digest) {
            return Err(invalid("blob digest mismatch"));
        }
        if self.path(&digest).is_file() {
            return Ok(digest);
        }
        if size > self.available() {
            return Err(invalid("CAS capacity exceeded"));
        }
        let mut temporary = tempfile::Builder::new()
            .prefix(".tmp")
            .tempfile_in(&self.root)?;
        io::copy(&mut File::open(source)?, &mut temporary)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(self.path(&digest))
            .map_err(|error| error.error)?;
        self.used += size;
        File::open(&self.root)?.sync_all()?;
        Ok(digest)
    }
}

pub(super) fn parse_digest(value: &str) -> io::Result<Digest> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("expected lowercase BLAKE3 digest"));
    }
    Digest::from_hex(value).ok_or_else(|| invalid("invalid digest"))
}

pub(super) struct LimitedWriter<W> {
    pub inner: W,
    pub remaining: u64,
}

impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(invalid("CAS capacity exceeded"));
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_reuses_and_bounds_blobs() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fs::write(&source, b"hello").unwrap();
        let mut cas = Cas::open(root.path().join("cas"), 4096).unwrap();
        assert!(cas
            .publish(&source, Some(Digest::of_bytes(b"wrong")))
            .is_err());
        let digest = cas.publish(&source, None).unwrap();
        assert_eq!(cas.publish(&source, None).unwrap(), digest);
        fs::write(&source, b"other").unwrap();
        assert!(cas.publish(&source, None).is_err());
        cas.make_room_pinned(5, &HashSet::from([digest])).unwrap();
        assert!(cas.path(&digest).exists());
        cas.make_room(5).unwrap();
        assert!(!cas.path(&digest).exists());
        assert!(cas.make_room(4097).is_err());
        fs::write(&source, b"").unwrap();
        cas.publish(&source, None).unwrap();
        assert_eq!(cas.available(), 0);
        assert_eq!(
            Cas::open(root.path().join("cas"), 4096)
                .unwrap()
                .available(),
            0
        );
        assert!(parse_digest("../escape").is_err());
        assert!(parse_digest(&"AA".repeat(32)).is_err());
    }
}
