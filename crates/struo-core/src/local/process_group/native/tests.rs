mod stress;

use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt};

use super::super::{registered, GroupChild};

#[allow(unsafe_code)]
fn available() -> bool {
    // SAFETY: glibc returns a process-lifetime, NUL-terminated version string.
    let version = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) };
    let mut components = version.to_str().unwrap().split('.');
    let major: u32 = components.next().unwrap().parse().unwrap();
    let minor: u32 = components.next().unwrap().parse().unwrap();
    (major, minor) >= (2, 29)
}

#[tokio::test]
async fn optional_chdir_support_selects_native_or_fallback() {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "printf '%s:%s' \"$PWD\" \"$CHECK\"; exit 17"])
        .env_clear()
        .env("CHECK", "isolated")
        .current_dir("/tmp");
    let native = GroupChild::spawn_native(&command, None, None).unwrap();
    assert_eq!(native.is_some(), available());
    let mut child = if let Some(child) = native {
        child
    } else {
        command.stdout(std::process::Stdio::piped());
        GroupChild::spawn(&mut command.into()).unwrap()
    };
    let mut stdout = String::new();
    child
        .take_stdout()
        .unwrap()
        .read_to_string(&mut stdout)
        .await
        .unwrap();
    assert_eq!(child.reap().await.unwrap().code(), Some(17));
    assert_eq!(stdout, "/tmp:isolated");
}

fn spawn(script: &str) -> GroupChild {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script]).env_clear().current_dir("/");
    GroupChild::spawn_native(&command, None, None)
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn termination_and_pipe_signals_are_not_left_blocked_or_ignored() {
    if !available() {
        return;
    }
    for (name, signal) in [
        ("TERM", libc::SIGTERM),
        ("INT", libc::SIGINT),
        ("PIPE", libc::SIGPIPE),
    ] {
        let mut child = spawn(&format!("kill -{name} $$; sleep 30"));
        let status = tokio::time::timeout(Duration::from_secs(3), child.reap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.signal(), Some(signal));
        assert_eq!(child.reap().await.unwrap(), status);
    }
}

#[test]
fn cancelling_reap_then_dropping_after_runtime_shutdown_reaps_the_leader() {
    if !available() {
        return;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (child, leader, descendant) = runtime.block_on(async {
        let mut child = spawn("sleep 30 & echo $$; echo $!; wait");
        let mut lines = tokio::io::BufReader::new(child.take_stdout().unwrap()).lines();
        let leader = lines.next_line().await.unwrap().unwrap();
        let descendant = lines.next_line().await.unwrap().unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), child.reap())
                .await
                .is_err()
        );
        assert!(registered(leader.parse().unwrap()));
        (child, leader, descendant)
    });
    drop(runtime);
    drop(child);
    assert!(!registered(leader.parse().unwrap()));
    for _ in 0..100 {
        let reaped = !std::path::Path::new(&format!("/proc/{leader}")).exists();
        let descendant_dead = std::fs::read_to_string(format!("/proc/{descendant}/stat"))
            .map_or(true, |stat| {
                stat.split(") ").nth(1).unwrap().starts_with('Z')
            });
        if reaped && descendant_dead {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("cancelled leader was not reaped or its descendant survived");
}

#[test]
#[allow(unsafe_code)]
fn the_child_preserves_the_original_nonempty_signal_mask() {
    if !available() {
        return;
    }
    std::thread::spawn(|| {
        // SAFETY: the mask is changed only on this disposable test thread.
        unsafe {
            let mut mask = std::mem::zeroed();
            libc::sigemptyset(&raw mut mask);
            libc::sigaddset(&raw mut mask, libc::SIGUSR1);
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_BLOCK, &raw const mask, std::ptr::null_mut()),
                0
            );
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let mut child = spawn("exec /bin/cat /proc/self/status");
            let mut stdout = String::new();
            child
                .take_stdout()
                .unwrap()
                .read_to_string(&mut stdout)
                .await
                .unwrap();
            assert!(child.reap().await.unwrap().success());
            let blocked = stdout
                .lines()
                .find_map(|line| line.strip_prefix("SigBlk:\t"))
                .unwrap();
            let blocked = u64::from_str_radix(blocked, 16).unwrap();
            assert_ne!(blocked & (1 << (libc::SIGUSR1 - 1)), 0);
            assert_eq!(
                blocked & ((1 << (libc::SIGINT - 1)) | (1 << (libc::SIGTERM - 1))),
                0
            );
        });
    })
    .join()
    .unwrap();
}
