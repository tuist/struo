#![allow(unsafe_code)]

use std::ffi::{CStr, CString};
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::sync::OnceLock;

fn check(code: libc::c_int) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}

pub(super) fn duplicate(fd: &impl AsRawFd) -> io::Result<OwnedFd> {
    // SAFETY: duplicates a borrowed live fd above stdio, without consuming it.
    let new = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
    if new < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fcntl returned a newly owned fd.
    Ok(unsafe { OwnedFd::from_raw_fd(new) })
}

pub(super) struct Actions(libc::posix_spawn_file_actions_t);

impl Actions {
    pub(super) fn new(cwd: &CStr, fds: &[OwnedFd; 3]) -> io::Result<Option<Self>> {
        type AddChdir = unsafe extern "C" fn(
            *mut libc::posix_spawn_file_actions_t,
            *const libc::c_char,
        ) -> libc::c_int;
        static ADD_CHDIR: OnceLock<Option<AddChdir>> = OnceLock::new();
        let Some(add_chdir) = ADD_CHDIR.get_or_init(|| {
            // SAFETY: resolve the optional glibc 2.29 function without raising
            // the executable's minimum glibc version. The symbol has this ABI.
            unsafe {
                let symbol = libc::dlsym(
                    libc::RTLD_DEFAULT,
                    c"posix_spawn_file_actions_addchdir_np".as_ptr(),
                );
                if symbol.is_null() {
                    None
                } else {
                    Some(std::mem::transmute::<*mut libc::c_void, AddChdir>(symbol))
                }
            }
        }) else {
            return Ok(None);
        };
        let mut actions = MaybeUninit::uninit();
        // SAFETY: initialization writes actions on success; the owner destroys it.
        unsafe {
            check(libc::posix_spawn_file_actions_init(actions.as_mut_ptr()))?;
            let mut actions = Self(actions.assume_init());
            for (fd, target) in fds.iter().zip(0..3) {
                check(libc::posix_spawn_file_actions_adddup2(
                    &raw mut actions.0,
                    fd.as_raw_fd(),
                    target,
                ))?;
            }
            check(add_chdir(&raw mut actions.0, cwd.as_ptr()))?;
            Ok(Some(actions))
        }
    }
}

impl Drop for Actions {
    fn drop(&mut self) {
        // SAFETY: successfully initialized, uniquely owned actions.
        unsafe {
            libc::posix_spawn_file_actions_destroy(&raw mut self.0);
        }
    }
}

pub(super) struct Attributes(libc::posix_spawnattr_t);

impl Attributes {
    pub(super) fn new(mask: &libc::sigset_t) -> io::Result<Self> {
        let mut attrs = MaybeUninit::uninit();
        // SAFETY: all pointers refer to initialized, live storage. Signal sets
        // are initialized before use. The owner destroys attrs on every exit.
        unsafe {
            check(libc::posix_spawnattr_init(attrs.as_mut_ptr()))?;
            let mut attrs = Self(attrs.assume_init());
            let mut defaults = MaybeUninit::uninit();
            libc::sigemptyset(defaults.as_mut_ptr());
            let mut defaults = defaults.assume_init();
            libc::sigaddset(&raw mut defaults, libc::SIGPIPE);
            check(libc::posix_spawnattr_setsigdefault(
                &raw mut attrs.0,
                &raw const defaults,
            ))?;
            check(libc::posix_spawnattr_setsigmask(&raw mut attrs.0, mask))?;
            check(libc::posix_spawnattr_setpgroup(&raw mut attrs.0, 0))?;
            check(libc::posix_spawnattr_setflags(
                &raw mut attrs.0,
                libc::c_short::try_from(
                    libc::POSIX_SPAWN_SETPGROUP
                        | libc::POSIX_SPAWN_SETSIGMASK
                        | libc::POSIX_SPAWN_SETSIGDEF,
                )
                .expect("spawn flags fit c_short"),
            ))?;
            Ok(attrs)
        }
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: successfully initialized, uniquely owned attributes.
        unsafe {
            libc::posix_spawnattr_destroy(&raw mut self.0);
        }
    }
}

pub(super) fn spawn(
    program: &CStr,
    argv: &[CString],
    env: &[CString],
    actions: &Actions,
    attrs: &Attributes,
) -> io::Result<libc::pid_t> {
    let pointers = |strings: &[CString]| {
        strings
            .iter()
            .map(|s| s.as_ptr().cast_mut())
            .chain(std::iter::once(std::ptr::null_mut()))
            .collect::<Vec<_>>()
    };
    let argv = pointers(argv);
    let env = pointers(env);
    let mut pid = 0;
    // SAFETY: strings and null-terminated pointer arrays stay alive through
    // spawn; actions and attrs are initialized. No Rust code runs in the child.
    unsafe {
        check(libc::posix_spawn(
            &raw mut pid,
            program.as_ptr(),
            &raw const actions.0,
            &raw const attrs.0,
            argv.as_ptr(),
            env.as_ptr(),
        ))?;
    }
    Ok(pid)
}

pub(super) fn try_wait(pid: libc::pid_t) -> io::Result<Option<ExitStatus>> {
    loop {
        let mut status = 0;
        // SAFETY: caller exclusively owns this positive, unreaped child pid.
        let result = unsafe { libc::waitpid(pid, &raw mut status, libc::WNOHANG) };
        if result > 0 {
            return Ok(Some(ExitStatus::from_raw(status)));
        }
        if result == 0 {
            return Ok(None);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

pub(super) fn kill(pid: libc::pid_t) {
    // SAFETY: caller exclusively owns this positive, unreaped child pid.
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
}
