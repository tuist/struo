use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path};

use super::super::cas::{invalid, Cas, LimitedWriter};
use super::super::sandbox::checked_path;
use super::super::{Input, InputKind};
use crate::WorkspacePath;

pub(super) fn prepare(root: &Path, cas: &Cas, request: &super::Request) -> io::Result<()> {
    fs::create_dir(root)?;
    materialize(root, cas, &request.inputs)?;
    for path in &request.clean_paths {
        if path.as_str().is_empty() {
            return Err(invalid("cannot clean workspace root"));
        }
        let path = checked_path(root, path, true)?;
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.is_dir() {
                fs::remove_dir_all(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
        }
    }
    for path in &request.create_dirs {
        fs::create_dir_all(checked_path(root, path, false)?)?;
    }
    materialize(root, cas, &request.post_setup_inputs)?;
    for output in &request.outputs {
        let path = checked_path(root, output, true)?;
        if let Some(parent) = path.parent().filter(|parent| parent.starts_with(root)) {
            fs::create_dir_all(parent)?;
        }
    }
    if let Some(cwd) = &request.cwd {
        if !checked_path(root, cwd, false)?.is_dir() {
            return Err(invalid("action cwd is not a directory"));
        }
    }
    Ok(())
}

pub(super) fn materialize(root: &Path, cas: &Cas, inputs: &[Input]) -> io::Result<()> {
    for input in inputs {
        let destination = checked_path(root, &input.path, false)?;
        if input.path.as_str().is_empty() && !matches!(input.kind, InputKind::Directory) {
            return Err(invalid("input cannot replace workspace root"));
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        match &input.kind {
            InputKind::Directory => fs::create_dir_all(&destination)?,
            InputKind::File { digest, executable } => {
                // Copy, never hardlink: writable action files must not alias CAS.
                fs::copy(cas.path(digest), &destination)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(
                        &destination,
                        fs::Permissions::from_mode(if *executable { 0o755 } else { 0o644 }),
                    )?;
                }
                #[cfg(not(unix))]
                let _ = executable;
            }
            InputKind::Symlink { target } => {
                safe_link(Path::new(input.path.as_str()), Path::new(target))?;
                #[cfg(unix)]
                std::os::unix::fs::symlink(target, destination)?;
                #[cfg(not(unix))]
                return Err(invalid("worker symlinks require Unix"));
            }
        }
    }
    Ok(())
}

fn safe_link(path: &Path, target: &Path) -> io::Result<()> {
    let mut depth = path.parent().map_or(0, |path| path.components().count());
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return Err(invalid("symlink escapes action workspace")),
        }
    }
    Ok(())
}

pub(super) fn archive(
    root: &Path,
    outputs: &[WorkspacePath],
    destination: &Path,
    limit: u64,
) -> io::Result<()> {
    let writer = LimitedWriter {
        inner: File::create(destination)?,
        remaining: limit,
    };
    let mut builder = tar::Builder::new(writer);
    builder.follow_symlinks(false);
    builder.sparse(false);
    let mut count = 0;
    for output in outputs {
        checked_path(root, output, true)?;
    }
    for output in crate::remote::output_roots(outputs) {
        let source = checked_path(root, &output, true)?;
        if source
            .symlink_metadata()
            .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
        {
            continue;
        }
        append(
            &mut builder,
            root,
            Path::new(output.as_str()),
            0,
            &mut count,
        )?;
    }
    builder.finish()?;
    builder.into_inner()?.flush()
}

fn append<W: Write>(
    builder: &mut tar::Builder<W>,
    root: &Path,
    path: &Path,
    depth: usize,
    count: &mut usize,
) -> io::Result<()> {
    *count += 1;
    if depth > 128 || *count > 100_000 {
        return Err(invalid("output tree exceeds metadata limit"));
    }
    let source = root.join(path);
    let metadata = source.symlink_metadata()?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(&source)?;
        safe_link(path, &target)?;
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o777);
        header.set_size(0);
        header.set_entry_type(tar::EntryType::Symlink);
        builder.append_link(&mut header, path, target)?;
    } else if metadata.is_file() {
        builder.append_file(path, &mut File::open(source)?)?;
    } else if metadata.is_dir() {
        if !path.as_os_str().is_empty() {
            builder.append_dir(path, &source)?;
        }
        for child in fs::read_dir(source)? {
            append(
                builder,
                root,
                &path.join(child?.file_name()),
                depth + 1,
                count,
            )?;
        }
    } else {
        return Err(invalid("unsupported output file type"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symlinks_cannot_escape_archive_or_materialization() {
        assert!(safe_link(Path::new("a/link"), Path::new("../file")).is_ok());
        assert!(safe_link(Path::new("link"), Path::new("../file")).is_err());
        assert!(safe_link(Path::new("a/link"), Path::new("/etc/passwd")).is_err());
    }

    #[test]
    fn sparse_outputs_use_portable_regular_entries() {
        use std::io::{Read, Seek, SeekFrom};

        let root = tempfile::tempdir().unwrap();
        let mut file = File::create(root.path().join("library.so")).unwrap();
        file.write_all(b"head").unwrap();
        file.seek(SeekFrom::Start(65532)).unwrap();
        file.write_all(b"tail").unwrap();
        drop(file);
        let path = root.path().join("outputs.tar");
        archive(
            root.path(),
            &["library.so".parse().unwrap()],
            &path,
            128 * 1024,
        )
        .unwrap();

        let mut archive = tar::Archive::new(File::open(path).unwrap());
        let mut entries = archive.entries().unwrap();
        let mut entry = entries.next().unwrap().unwrap();
        assert!(entry.header().entry_type().is_file());
        let mut actual = Vec::new();
        entry.read_to_end(&mut actual).unwrap();
        let mut expected = vec![0; 65536];
        expected[..4].copy_from_slice(b"head");
        expected[65532..].copy_from_slice(b"tail");
        assert_eq!(actual, expected);
        drop(entry);
        assert!(entries.next().is_none());
    }

    #[test]
    fn workspace_copy_does_not_mutate_reusable_blob() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fs::write(&source, "original").unwrap();
        let mut cas = Cas::open(root.path().join("cas"), 4096).unwrap();
        let digest = cas.publish(&source, None).unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        materialize(
            &workspace,
            &cas,
            &[Input {
                path: "file".parse().unwrap(),
                kind: InputKind::File {
                    digest,
                    executable: false,
                },
            }],
        )
        .unwrap();
        fs::write(workspace.join("file"), "changed").unwrap();
        assert_eq!(fs::read_to_string(cas.path(&digest)).unwrap(), "original");
    }
}
