mod reaper;
mod sys;

#[cfg(test)]
mod tests;

use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::process::{Command, ExitStatus};

use tokio::process::{ChildStderr, ChildStdout};

use super::{registry, sys::TerminationSignalsHeld, GroupChild, Leader};

pub(super) struct Child {
    owner: reaper::Owner,
    status: Option<ExitStatus>,
    pub(super) stdout: Option<ChildStdout>,
    pub(super) stderr: Option<ChildStderr>,
}

impl Child {
    pub(super) fn reap(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.status {
            return Ok(status);
        }
        let status = self.owner.reap()?;
        self.status = Some(status);
        Ok(status)
    }
}

fn cstring(value: &OsStr) -> io::Result<CString> {
    CString::new(value.as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

fn output(file: Option<&File>) -> io::Result<(OwnedFd, Option<OwnedFd>)> {
    if let Some(file) = file {
        return Ok((sys::duplicate(file)?, None));
    }
    let (read, write) = io::pipe()?;
    Ok((sys::duplicate(&write)?, Some(read.into())))
}

fn search_path(
    command: &Command,
    run: impl Fn(&std::ffi::CStr) -> io::Result<libc::pid_t>,
) -> io::Result<libc::pid_t> {
    let path = command
        .get_envs()
        .find_map(|(key, value)| (key == "PATH").then_some(value).flatten())
        .unwrap_or(OsStr::new("/bin:/usr/bin"));
    let mut error = io::Error::from_raw_os_error(libc::ENOENT);
    let mut denied = false;
    for directory in std::env::split_paths(path) {
        error = match run(&cstring(directory.join(command.get_program()).as_os_str())?) {
            Ok(pid) => return Ok(pid),
            Err(error) => error,
        };
        match error.raw_os_error() {
            Some(libc::EACCES) => denied = true,
            Some(libc::ENOENT | libc::ENOTDIR | libc::ESTALE | libc::ENODEV | libc::ETIMEDOUT) => {}
            _ => return Err(error),
        }
    }
    Err(if denied {
        io::Error::from_raw_os_error(libc::EACCES)
    } else {
        error
    })
}

impl GroupChild {
    // Only the local invocation builder may use this: Command's opaque
    // pre_exec hooks and stdio cannot be inspected or reproduced here.
    pub(in crate::local) fn spawn_native(
        command: &Command,
        stdout: Option<&File>,
        stderr: Option<&File>,
    ) -> io::Result<Option<Self>> {
        let args = std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(cstring)
            .collect::<io::Result<Vec<_>>>()?;
        if args[0].is_empty() {
            return Err(io::Error::from_raw_os_error(libc::ENOENT));
        }
        let env = command
            .get_envs()
            .filter_map(|(key, value)| value.map(|value| (key, value)))
            .map(|(key, value)| {
                let mut entry = key.as_bytes().to_vec();
                if entry.contains(&b'=') {
                    return Err(io::ErrorKind::InvalidInput.into());
                }
                entry.push(b'=');
                entry.extend_from_slice(value.as_bytes());
                CString::new(entry)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let cwd = cstring(
            command
                .get_current_dir()
                .expect("local invocation has cwd")
                .as_os_str(),
        )?;
        let (stdout_fd, stdout) = output(stdout)?;
        let (stderr_fd, stderr) = output(stderr)?;
        let stdin = sys::duplicate(&File::open("/dev/null")?)?;
        let fds = [stdin, stdout_fd, stderr_fd];
        let Some(actions) = sys::Actions::new(&cwd, &fds)? else {
            return Ok(None);
        };
        let stdout = stdout
            .map(|fd| ChildStdout::from_std(fd.into()))
            .transpose()?;
        let stderr = stderr
            .map(|fd| ChildStderr::from_std(fd.into()))
            .transpose()?;
        let mut owner = reaper::Owner::reserve()?;
        let held = TerminationSignalsHeld::new()?;
        let mut group = registry::Registration::reserve()?;
        let attrs = sys::Attributes::new(held.previous())?;
        let run = |program: &std::ffi::CStr| sys::spawn(program, &args, &env, &actions, &attrs);
        let result = if args[0].as_bytes().contains(&b'/') {
            run(&args[0])
        } else {
            search_path(command, run)
        };
        let pid = match result {
            Ok(pid) => pid,
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ENOEXEC | libc::ENAMETOOLONG)
                ) =>
            {
                return Ok(None)
            }
            Err(error) => return Err(error),
        };
        owner.assign(pid);
        let child = Child {
            owner,
            status: None,
            stdout,
            stderr,
        };
        group.assign(Some(
            u32::try_from(pid).expect("spawn returns a positive pid"),
        ))?;
        drop(held);
        Ok(Some(Self {
            child: Leader::Native(child),
            group: Some(group),
        }))
    }
}
