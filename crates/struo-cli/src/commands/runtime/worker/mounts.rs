#![cfg(target_os = "linux")]
#![allow(unsafe_code)]

use std::ffi::CString;
use std::io::Read;

use anyhow::{ensure, Context, Result};

pub(crate) fn prepare() -> Result<()> {
    let uid = std::env::var("SUDO_UID")?.parse::<libc::uid_t>()?;
    let gid = std::env::var("SUDO_GID")?.parse::<libc::gid_t>()?;
    ensure!(
        uid != 0,
        "private worker mounts require a non-root invoking user"
    );
    let mut mounts = String::new();
    std::fs::File::open("/proc/self/mountinfo")?
        .take(1_048_577)
        .read_to_string(&mut mounts)?;
    ensure!(
        mounts.len() <= 1_048_576,
        "worker mount metadata exceeds 1 MiB"
    );
    let mut paths: Vec<_> = mounts
        .lines()
        .filter_map(|line| line.split_whitespace().nth(4))
        .filter(|path| path.starts_with("/proc/"))
        .collect();
    paths.sort_unstable_by_key(|path| std::cmp::Reverse(path.len()));
    // SAFETY: this runs before the runtime creates threads. Each syscall uses
    // constant flags or a live, NUL-terminated path and changes only this process.
    unsafe {
        check(libc::unshare(libc::CLONE_NEWNS | libc::CLONE_FS))
            .context("creating private worker mount namespace")?;
        check(libc::mount(
            std::ptr::null(),
            c"/".as_ptr(),
            std::ptr::null(),
            libc::MS_REC | libc::MS_PRIVATE,
            std::ptr::null(),
        ))
        .context("preventing worker mount propagation")?;
        for path in paths {
            ensure!(!path.contains('\\'), "unsupported escaped proc mount path");
            let path = CString::new(path)?;
            check(libc::umount2(path.as_ptr(), 0)).context("preparing private parent proc view")?;
        }
        check(libc::setgroups(0, std::ptr::null())).context("dropping worker groups")?;
        check(libc::setgid(gid)).context("dropping worker group")?;
        check(libc::setuid(uid)).context("dropping worker privileges")?;
    }
    Ok(())
}

fn check(result: libc::c_int) -> std::io::Result<()> {
    if result == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
