//! ptrace backend for Linux on x86-64 and arm64.
//!
//! The helper starts the command with `PTRACE_TRACEME`, follows every fork,
//! vfork, clone, and exec, and stops the tree at each syscall entry and exit.
//! File syscalls are recorded with their path, resolved against the calling
//! thread's working directory or directory descriptor at entry, and whether
//! they succeeded. `io_uring_setup` fails with `ENOSYS`, because `io_uring`
//! opens and reads files without syscalls the tracer sees. When the command
//! exits, the helper exits and `PTRACE_O_EXITKILL` kills any tracee left.
//!
//! This module is a sanctioned FFI boundary for `unsafe`: ptrace, waitpid,
//! and `process_vm_readv` have no safe wrappers in this workspace.
#![allow(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::os::unix::process::CommandExt as _;
use std::path::PathBuf;

use super::{exit_code, run_untraced, Event, Log, Op, Spec};

#[cfg(target_arch = "aarch64")]
#[path = "linux/aarch64.rs"]
mod arch;
#[cfg(target_arch = "x86_64")]
#[path = "linux/x86_64.rs"]
mod arch;

const PAGE: usize = 4096;
const PATH_MAX: usize = 4096;

pub(super) fn trace(spec: &Spec, command: &[OsString], log: &mut Log) -> std::io::Result<i32> {
    let mut traced = std::process::Command::new(&command[0]);
    traced.args(&command[1..]);
    // SAFETY: the closure only issues the async-signal-safe `ptrace` syscall
    // between fork and exec.
    unsafe {
        traced.pre_exec(|| {
            if libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = match traced.spawn() {
        Ok(child) => child,
        Err(error) => {
            log.write(&Event::Unavailable {
                reason: format!("ptrace could not start the command: {error}"),
            })?;
            return run_untraced(command);
        }
    };
    let root = libc::pid_t::try_from(child.id()).map_err(std::io::Error::other)?;
    let mut status = 0;
    wait(root, &mut status)?;
    if !libc::WIFSTOPPED(status) {
        log.write(&Event::Unavailable {
            reason: "the command did not stop for the tracer".to_string(),
        })?;
        return Ok(wait_status_code(status));
    }
    let options = libc::PTRACE_O_TRACESYSGOOD
        | libc::PTRACE_O_TRACEFORK
        | libc::PTRACE_O_TRACEVFORK
        | libc::PTRACE_O_TRACECLONE
        | libc::PTRACE_O_TRACEEXEC
        | libc::PTRACE_O_EXITKILL;
    // SAFETY: `root` is our stopped tracee.
    if unsafe { libc::ptrace(libc::PTRACE_SETOPTIONS, root, 0, options) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    log.write(&Event::Started {
        backend: "ptrace".to_string(),
        execroot: spec.execroot.clone(),
    })?;
    if let Ok(exe) = std::fs::read_link(format!("/proc/{root}/exe")) {
        log.write(&Event::Access {
            op: Op::Exec,
            path: exe,
            ok: true,
        })?;
    }
    Tracer::new(root, log).run()
}

struct Pending {
    op: Op,
    path: PathBuf,
    socket: bool,
}

struct Tracer<'a> {
    root: libc::pid_t,
    log: &'a mut Log,
    started: HashSet<libc::pid_t>,
    in_syscall: HashMap<libc::pid_t, bool>,
    pending: HashMap<libc::pid_t, Pending>,
}

impl<'a> Tracer<'a> {
    fn new(root: libc::pid_t, log: &'a mut Log) -> Self {
        Self {
            root,
            log,
            started: HashSet::from([root]),
            in_syscall: HashMap::new(),
            pending: HashMap::new(),
        }
    }

    fn run(mut self) -> std::io::Result<i32> {
        resume(self.root, 0);
        loop {
            let mut status = 0;
            // SAFETY: plain waitpid on any traced child.
            let pid = unsafe { libc::waitpid(-1, &raw mut status, libc::__WALL) };
            if pid == -1 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                return Err(error);
            }
            if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
                if pid == self.root {
                    return Ok(wait_status_code(status));
                }
                self.forget(pid);
                continue;
            }
            if !libc::WIFSTOPPED(status) {
                continue;
            }
            let signal = libc::WSTOPSIG(status);
            let event = (status >> 16) & 0xff;
            if signal == (libc::SIGTRAP | 0x80) {
                let entering = !self.in_syscall.get(&pid).copied().unwrap_or(false);
                self.in_syscall.insert(pid, entering);
                if entering {
                    self.enter(pid);
                } else {
                    self.exit(pid)?;
                }
                resume(pid, 0);
            } else if event != 0 {
                resume(pid, 0);
            } else if signal == libc::SIGSTOP && self.started.insert(pid) {
                // A new tracee's first stop; the signal is the tracer's, not the program's.
                resume(pid, 0);
            } else {
                resume(pid, signal);
            }
        }
    }

    fn forget(&mut self, pid: libc::pid_t) {
        self.started.remove(&pid);
        self.in_syscall.remove(&pid);
        self.pending.remove(&pid);
    }

    fn enter(&mut self, pid: libc::pid_t) {
        let Some((call, [a, b, c])) = arch::entry(pid) else {
            return;
        };
        let pending = match call {
            #[cfg(target_arch = "x86_64")]
            libc::SYS_open => open(pid, libc::AT_FDCWD, a, b),
            libc::SYS_openat => open(pid, dirfd(a), b, c),
            libc::SYS_openat2 => {
                let flags = read_u64(pid, c).unwrap_or(0);
                open(pid, dirfd(a), b, flags)
            }
            #[cfg(target_arch = "x86_64")]
            libc::SYS_stat | libc::SYS_lstat | libc::SYS_access | libc::SYS_readlink => {
                at(pid, libc::AT_FDCWD, a, Op::Probe)
            }
            libc::SYS_newfstatat
            | libc::SYS_statx
            | libc::SYS_faccessat
            | libc::SYS_faccessat2
            | libc::SYS_readlinkat => at(pid, dirfd(a), b, Op::Probe),
            libc::SYS_execve => at(pid, libc::AT_FDCWD, a, Op::Exec),
            libc::SYS_execveat => at(pid, dirfd(a), b, Op::Exec),
            libc::SYS_getdents64 => fd_path(pid, dirfd(a)).map(|path| Pending {
                op: Op::List,
                path,
                socket: false,
            }),
            libc::SYS_connect => socket_address(pid, b, c).map(|address| Pending {
                op: Op::Probe,
                path: PathBuf::from(address),
                socket: true,
            }),
            libc::SYS_io_uring_setup => {
                arch::skip(pid);
                None
            }
            _ => None,
        };
        match pending {
            Some(pending) => {
                self.pending.insert(pid, pending);
            }
            None => {
                self.pending.remove(&pid);
            }
        }
    }

    fn exit(&mut self, pid: libc::pid_t) -> std::io::Result<()> {
        let Some(pending) = self.pending.remove(&pid) else {
            return Ok(());
        };
        // An exec that succeeded replaced the image; its result is 0 or the
        // tracee is already in the new program, so treat both as success.
        let ok = arch::result(pid).is_none_or(|result| result >= 0);
        if pending.socket {
            if ok {
                self.log.write(&Event::Socket {
                    address: pending.path.display().to_string(),
                })?;
            }
            return Ok(());
        }
        self.log.write(&Event::Access {
            op: pending.op,
            path: pending.path,
            ok,
        })
    }
}

fn resume(pid: libc::pid_t, signal: libc::c_int) {
    // SAFETY: restarts a stopped tracee; failure means it already exited.
    unsafe {
        libc::ptrace(libc::PTRACE_SYSCALL, pid, 0, signal);
    }
}

fn wait(pid: libc::pid_t, status: &mut libc::c_int) -> std::io::Result<()> {
    loop {
        // SAFETY: waits for our own child.
        if unsafe { libc::waitpid(pid, status, libc::__WALL) } != -1 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINTR) {
            return Err(error);
        }
    }
}

fn wait_status_code(status: libc::c_int) -> i32 {
    exit_code(std::os::unix::process::ExitStatusExt::from_raw(status))
}

fn dirfd(register: u64) -> libc::c_int {
    // The kernel reads directory descriptors as 32-bit ints.
    #[allow(clippy::cast_possible_truncation)]
    let fd = register as u32;
    fd.cast_signed()
}

fn open(pid: libc::pid_t, dirfd: libc::c_int, path: u64, flags: u64) -> Option<Pending> {
    #[allow(clippy::cast_possible_truncation)]
    let flags = flags as libc::c_int;
    if flags & libc::O_ACCMODE == libc::O_WRONLY {
        return None;
    }
    let op = if flags & libc::O_DIRECTORY != 0 {
        Op::Probe
    } else {
        Op::Read
    };
    at(pid, dirfd, path, op)
}

fn at(pid: libc::pid_t, dirfd: libc::c_int, path: u64, op: Op) -> Option<Pending> {
    let path = read_string(pid, path)?;
    if path.as_os_str().is_empty() {
        return None;
    }
    let path = if path.is_absolute() {
        path
    } else {
        let base = if dirfd == libc::AT_FDCWD {
            std::fs::read_link(format!("/proc/{pid}/cwd")).ok()?
        } else {
            fd_path(pid, dirfd)?
        };
        base.join(path)
    };
    Some(Pending {
        op,
        path: super::normalize(&path),
        socket: false,
    })
}

fn fd_path(pid: libc::pid_t, fd: libc::c_int) -> Option<PathBuf> {
    let path = std::fs::read_link(format!("/proc/{pid}/fd/{fd}")).ok()?;
    path.is_absolute().then_some(path)
}

fn socket_address(pid: libc::pid_t, address: u64, length: u64) -> Option<String> {
    let length = usize::try_from(length).ok()?.min(256);
    let bytes = read_bytes(pid, address, length)?;
    let family = u16::from_ne_bytes([*bytes.first()?, *bytes.get(1)?]);
    match libc::c_int::from(family) {
        libc::AF_UNIX => {
            let path = &bytes[2..];
            let end = if path.first() == Some(&0) {
                // An abstract socket: the name follows the leading NUL.
                return Some(format!(
                    "unix:@{}",
                    String::from_utf8_lossy(&path[1..]).trim_end_matches('\0')
                ));
            } else {
                path.iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(path.len())
            };
            Some(format!("unix:{}", String::from_utf8_lossy(&path[..end])))
        }
        libc::AF_INET | libc::AF_INET6 => Some(format!("inet family {family}")),
        _ => None,
    }
}

fn read_u64(pid: libc::pid_t, address: u64) -> Option<u64> {
    let bytes = read_bytes(pid, address, 8)?;
    Some(u64::from_ne_bytes(bytes.try_into().ok()?))
}

fn read_string(pid: libc::pid_t, address: u64) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt as _;
    let mut bytes = Vec::new();
    let mut cursor = address;
    while bytes.len() < PATH_MAX {
        // Stay within one page per read: the next page may be unmapped.
        let page_left = PAGE - usize::try_from(cursor).ok()? % PAGE;
        let chunk = read_bytes(pid, cursor, page_left)?;
        if let Some(end) = chunk.iter().position(|&byte| byte == 0) {
            bytes.extend_from_slice(&chunk[..end]);
            return Some(PathBuf::from(OsString::from_vec(bytes)));
        }
        bytes.extend_from_slice(&chunk);
        cursor += page_left as u64;
    }
    None
}

fn read_bytes(pid: libc::pid_t, address: u64, length: usize) -> Option<Vec<u8>> {
    if address == 0 {
        return None;
    }
    let mut buffer = vec![0u8; length];
    let local = libc::iovec {
        iov_base: buffer.as_mut_ptr().cast(),
        iov_len: length,
    };
    let remote = libc::iovec {
        iov_base: usize::try_from(address).ok()? as *mut libc::c_void,
        iov_len: length,
    };
    // SAFETY: copies at most `length` bytes into `buffer`, which owns them.
    let read = unsafe { libc::process_vm_readv(pid, &raw const local, 1, &raw const remote, 1, 0) };
    let read = usize::try_from(read).ok()?;
    buffer.truncate(read);
    Some(buffer)
}
