use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Record, MAX_SLOTS};

const MAX_RECORD_BYTES: u64 = 16 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u32,
    #[serde(deserialize_with = "Option::deserialize")]
    record: Option<Record>,
}

pub(super) fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(super) fn create_root(root: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(root)?;
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() {
        return Err(invalid("worker pool root must be a directory"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
        // A synced record must not disappear with a newly created ancestor.
        for directory in root.ancestors() {
            File::open(if directory.as_os_str().is_empty() {
                Path::new(".")
            } else {
                directory
            })?
            .sync_all()?;
        }
    }
    Ok(())
}

fn regular(path: &Path) -> io::Result<()> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(invalid("worker state must be a regular file"));
    }
    Ok(())
}

pub(super) fn open_lock(path: &Path) -> io::Result<File> {
    match regular(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

pub(super) fn indices(root: &Path) -> io::Result<Vec<usize>> {
    let mut indices = BTreeSet::new();
    for (count, entry) in fs::read_dir(root)?.enumerate() {
        if count >= MAX_SLOTS * 3 + 16 {
            return Err(invalid("too many worker pool directory entries"));
        }
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| invalid("invalid worker state filename"))?;
        if name == "capacity.lock" || name.starts_with(".tmp") {
            continue;
        }
        let (index, extension) = name
            .split_once('.')
            .ok_or_else(|| invalid("unknown worker state"))?;
        let index: usize = index
            .parse()
            .map_err(|_| invalid("invalid worker slot index"))?;
        if index >= MAX_SLOTS
            || !matches!(extension, "lock" | "json")
            || name != format!("{index}.{extension}")
        {
            return Err(invalid("invalid worker slot filename"));
        }
        regular(&entry.path())?;
        indices.insert(index);
    }
    Ok(indices.into_iter().collect())
}

pub(super) fn read(path: &Path) -> io::Result<Option<Record>> {
    regular(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut bytes = Vec::new();
    options
        .open(path)?
        .take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(invalid("oversized worker record"));
    }
    let stored: Stored = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if stored.version != 1 {
        return Err(invalid("unsupported worker record version"));
    }
    if let Some(record) = &stored.record {
        record.validate()?;
    }
    Ok(stored.record)
}

pub(super) fn write(root: &Path, index: usize, record: Option<&Record>) -> io::Result<()> {
    if let Some(record) = record {
        record.validate()?;
    }
    let bytes = serde_json::to_vec(&Stored {
        version: 1,
        record: record.cloned(),
    })
    .map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(invalid("oversized worker record"));
    }
    let mut temporary = tempfile::Builder::new().prefix(".tmp").tempfile_in(root)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(root.join(format!("{index}.json")))
        .map_err(|error| error.error)?;
    #[cfg(unix)]
    File::open(root)?.sync_all()?;
    Ok(())
}
