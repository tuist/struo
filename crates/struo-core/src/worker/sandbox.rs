use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[cfg(target_os = "linux")]
use libc::{PR_CAP_AMBIENT, PR_CAP_AMBIENT_CLEAR_ALL};
use tokio::process::Command;

use super::cas::invalid;
use super::{ServeOptions, EXECUTION_ROOT};

pub(super) fn command(
    options: &ServeOptions,
    workspace: &Path,
    argv: &[String],
    env: &BTreeMap<String, String>,
    cwd: &str,
) -> io::Result<Command> {
    if !cfg!(target_os = "linux") {
        return Err(invalid("worker isolation requires Linux bubblewrap"));
    }
    let executable = ["/usr/bin/bwrap", "/bin/bwrap"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
        .ok_or_else(|| invalid("worker isolation requires bubblewrap; no unisolated fallback"))?;
    let mut command = Command::new(executable);
    command.env_clear();
    // Provider services may inherit ambient capabilities that bubblewrap rejects.
    #[cfg(target_os = "linux")]
    #[allow(unsafe_code)]
    // SAFETY: only prctl with integer arguments runs between fork and exec.
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.args([
        "--die-with-parent",
        "--unshare-all",
        "--new-session",
        "--cap-drop",
        "ALL",
    ]);
    for path in ["/usr", "/bin", "/sbin", "/lib", "/lib64"] {
        if Path::new(path).exists() {
            command.args(["--ro-bind", path, path]);
        }
    }
    for path in [
        "/etc/alternatives",
        "/etc/ld.so.cache",
        "/etc/ld.so.conf",
        "/etc/ld.so.conf.d",
        "/etc/ssl/certs",
        "/etc/localtime",
    ] {
        if Path::new(path).exists() {
            command.args(["--ro-bind", path, path]);
        }
    }
    command.args([
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
        "--tmpfs",
        "/run",
        "--dir",
        "/home/sprite",
        "--dir",
        "/private-home",
    ]);
    let preparation = std::fs::canonicalize(&options.preparation_root)?;
    if !preparation.is_absolute() {
        return Err(invalid("preparation root must be absolute"));
    }
    // Never bind the preparation parent: it also holds credentials and CAS.
    for name in ["mise", "data", "env", "system.toml"] {
        let path = preparation.join(name);
        if path.exists() {
            let source = std::fs::canonicalize(&path)?;
            if !source.starts_with(&preparation) {
                return Err(invalid("prepared tool path escapes preparation root"));
            }
            command.arg("--ro-bind").arg(&source).arg(&path);
        }
    }
    // Prepared environment scratch paths are fresh mounts, never host binds.
    for value in options.tool_environment.values() {
        let path = Path::new(value);
        if path.is_absolute()
            && path.starts_with(&preparation)
            && path != preparation
            && !["mise", "data", "env", "system.toml"]
                .into_iter()
                .any(|name| path.starts_with(preparation.join(name)))
        {
            if path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
            {
                return Err(invalid("prepared scratch path escapes preparation root"));
            }
            command.arg("--tmpfs").arg(path);
        }
    }
    command.arg("--bind").arg(workspace).arg(EXECUTION_ROOT);
    command.args(["--chdir", cwd]);
    apply_environment(&mut command, options, env);
    command.arg("--").args(argv);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(command)
}

fn apply_environment(
    command: &mut Command,
    options: &ServeOptions,
    env: &BTreeMap<String, String>,
) {
    for (key, value) in env {
        command.args(["--setenv", key, value]);
    }
    // Keep mutable tool state separate from immutable installation data.
    command.args(["--dir", "/private-home/.state"]);
    for key in options.tool_environment.keys() {
        if let Some(prefix) = key.strip_suffix("_DATA_DIR") {
            let key = format!("{prefix}_STATE_DIR");
            if !env.contains_key(&key) {
                command.args(["--setenv", &key, "/private-home/.state"]);
            }
        }
    }
    for (key, value) in [
        ("HOME", "/private-home"),
        ("TMPDIR", "/tmp"),
        ("XDG_CACHE_HOME", "/private-home/.cache"),
        ("XDG_DATA_HOME", "/private-home/.local/share"),
        ("XDG_CONFIG_HOME", "/private-home/.config"),
    ] {
        if !env.contains_key(key) {
            command.args(["--setenv", key, value]);
        }
    }
}

pub(super) fn checked_path(
    root: &Path,
    path: &crate::WorkspacePath,
    leaf_symlink: bool,
) -> io::Result<PathBuf> {
    let mut current = root.to_path_buf();
    let components: Vec<_> = Path::new(path.as_str()).components().collect();
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if !leaf_symlink || index + 1 != components.len() {
                    return Err(invalid("workspace path traverses symlink"));
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn rejects_parent_symlinks_even_with_safe_leaf() {
        let root = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/tmp", root.path().join("link")).unwrap();
        assert!(checked_path(root.path(), &"link/file".parse().unwrap(), true).is_err());
        assert!(checked_path(root.path(), &"link".parse().unwrap(), false).is_err());
        assert!(checked_path(root.path(), &"link".parse().unwrap(), true).is_ok());
    }
}
