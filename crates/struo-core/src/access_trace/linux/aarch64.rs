//! arm64 registers: the syscall number is in `x8`, arguments in `x0` to
//! `x2`, and the result in `x0`. arm64 has no `PTRACE_GETREGS`; registers
//! are read with `PTRACE_GETREGSET`, and the syscall number is changed
//! through its own regset, because writing `x8` does not change it.

/// The regset holding the syscall number, from the kernel's `elf.h`.
const NT_ARM_SYSTEM_CALL: libc::c_int = 0x404;

pub(super) fn entry(pid: libc::pid_t) -> Option<(i64, [u64; 3])> {
    let regs = registers(pid)?;
    Some((
        regs.regs[8].cast_signed(),
        [regs.regs[0], regs.regs[1], regs.regs[2]],
    ))
}

pub(super) fn result(pid: libc::pid_t) -> Option<i64> {
    registers(pid).map(|regs| regs.regs[0].cast_signed())
}

/// Skip the syscall at entry. Syscall number -1 makes the kernel skip it but
/// keep `x0`, which still holds the first argument, so `x0` is set to
/// `-ENOSYS` first.
pub(super) fn skip(pid: libc::pid_t) {
    let Some(mut regs) = registers(pid) else {
        return;
    };
    regs.regs[0] = i64::from(-libc::ENOSYS).cast_unsigned();
    let mut iov = libc::iovec {
        iov_base: (&raw mut regs).cast(),
        iov_len: std::mem::size_of::<libc::user_regs_struct>(),
    };
    let mut number: libc::c_int = -1;
    let mut number_iov = libc::iovec {
        iov_base: (&raw mut number).cast(),
        iov_len: std::mem::size_of::<libc::c_int>(),
    };
    // SAFETY: both iovecs describe live, correctly sized buffers for a
    // stopped tracee's regsets.
    unsafe {
        libc::ptrace(libc::PTRACE_SETREGSET, pid, libc::NT_PRSTATUS, &raw mut iov);
        libc::ptrace(
            libc::PTRACE_SETREGSET,
            pid,
            NT_ARM_SYSTEM_CALL,
            &raw mut number_iov,
        );
    }
}

fn registers(pid: libc::pid_t) -> Option<libc::user_regs_struct> {
    let mut regs = std::mem::MaybeUninit::<libc::user_regs_struct>::uninit();
    let mut iov = libc::iovec {
        iov_base: regs.as_mut_ptr().cast(),
        iov_len: std::mem::size_of::<libc::user_regs_struct>(),
    };
    // SAFETY: PTRACE_GETREGSET writes at most `iov_len` bytes into `regs`.
    let result =
        unsafe { libc::ptrace(libc::PTRACE_GETREGSET, pid, libc::NT_PRSTATUS, &raw mut iov) };
    // SAFETY: the kernel filled the whole struct when it reported its size.
    (result != -1 && iov.iov_len == std::mem::size_of::<libc::user_regs_struct>())
        .then(|| unsafe { regs.assume_init() })
}
