//! x86-64 registers: the syscall number is in `orig_rax`, arguments in
//! `rdi`, `rsi`, `rdx`, and the result in `rax`.

pub(super) fn entry(pid: libc::pid_t) -> Option<(i64, [u64; 3])> {
    let regs = registers(pid)?;
    Some((regs.orig_rax.cast_signed(), [regs.rdi, regs.rsi, regs.rdx]))
}

pub(super) fn result(pid: libc::pid_t) -> Option<i64> {
    registers(pid).map(|regs| regs.rax.cast_signed())
}

/// Skip the syscall at entry. An invalid number makes the kernel skip it and
/// leave the `-ENOSYS` it preloaded in `rax`.
pub(super) fn skip(pid: libc::pid_t) {
    let Some(mut regs) = registers(pid) else {
        return;
    };
    regs.orig_rax = u64::MAX;
    // SAFETY: writes back registers read from the same stopped tracee.
    unsafe {
        libc::ptrace(libc::PTRACE_SETREGS, pid, 0, &raw const regs);
    }
}

fn registers(pid: libc::pid_t) -> Option<libc::user_regs_struct> {
    let mut regs = std::mem::MaybeUninit::<libc::user_regs_struct>::uninit();
    // SAFETY: PTRACE_GETREGS fills the whole struct for a stopped tracee.
    let result = unsafe { libc::ptrace(libc::PTRACE_GETREGS, pid, 0, regs.as_mut_ptr()) };
    // SAFETY: initialized when the call succeeded.
    (result != -1).then(|| unsafe { regs.assume_init() })
}
