#![allow(unsafe_code)]

use std::process::{Command, Stdio};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use tokio::io::AsyncBufReadExt;

use super::super::super::{signal_action_process_groups, GroupChild};

const MODE: &str = "STRUO_NATIVE_CANCELLATION_STRESS";

#[test]
fn concurrent_spawn_cancellation_and_reaping() {
    if !super::available() {
        return;
    }
    for mode in ["drop", "INT", "TERM"] {
        let rounds = if mode == "drop" { 1 } else { 16 };
        for round in 0..rounds {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "local::process_group::native::tests::stress::child",
                    "--ignored",
                    "--nocapture",
                ])
                .env(MODE, mode)
                .env("STRUO_NATIVE_STRESS_DELAY_US", (round * 100).to_string())
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success(), "{mode} round {round}: {status}");
                    break;
                }
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("{mode} round {round} deadlocked");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

extern "C" fn terminate(signal: libc::c_int) {
    signal_action_process_groups(signal);
}

#[test]
#[ignore = "isolated process used by concurrent_spawn_cancellation_and_reaping"]
fn child() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    // SAFETY: this isolated test process owns its signal dispositions and
    // adopts action descendants so the test does not leave zombies to init.
    unsafe {
        assert_eq!(libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0), 0);
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = terminate as *const () as usize;
        libc::sigemptyset(&raw mut action.sa_mask);
        assert_eq!(
            libc::sigaction(libc::SIGINT, &raw const action, std::ptr::null_mut()),
            0
        );
        assert_eq!(
            libc::sigaction(libc::SIGTERM, &raw const action, std::ptr::null_mut()),
            0
        );
    }
    let barrier = Arc::new(Barrier::new(5));
    let finish = Arc::new(Barrier::new(5));
    let (send, receive) = std::sync::mpsc::channel();
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let (barrier, finish, send, mode) =
                (barrier.clone(), finish.clone(), send.clone(), mode.clone());
            std::thread::spawn(move || {
                // SAFETY: the finish barrier keeps this thread alive until
                // every pthread_kill has returned.
                send.send(unsafe { libc::pthread_self() }).unwrap();
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                barrier.wait();
                let leaders = runtime.block_on(exercise_actions(&mode));
                drop(runtime);
                finish.wait();
                leaders
            })
        })
        .collect();
    let threads: Vec<_> = (0..4).map(|_| receive.recv().unwrap()).collect();
    barrier.wait();
    if mode != "drop" {
        let delay = std::env::var("STRUO_NATIVE_STRESS_DELAY_US")
            .unwrap()
            .parse()
            .unwrap();
        std::thread::sleep(Duration::from_micros(delay));
        for thread in threads {
            // SAFETY: each thread remains alive at the finish barrier.
            assert_eq!(
                unsafe {
                    libc::pthread_kill(
                        thread,
                        if mode == "INT" {
                            libc::SIGINT
                        } else {
                            libc::SIGTERM
                        },
                    )
                },
                0
            );
        }
    }
    finish.wait();
    let leaders: Vec<_> = workers
        .into_iter()
        .flat_map(|worker| worker.join().unwrap())
        .collect();
    assert_reaped(&leaders);
}

async fn exercise_actions(mode: &str) -> Vec<i32> {
    let mut leaders = Vec::new();
    for iteration in 0..128 {
        let mut command = Command::new("/bin/sh");
        let script = if mode == "drop" {
            if iteration % 3 == 0 {
                "sleep 30 & echo $$; exit 0"
            } else {
                "sleep 30 & echo $$; wait"
            }
        } else {
            "echo $$; exec sleep 30"
        };
        command.args(["-c", script]).env_clear().current_dir("/");
        let mut child = match GroupChild::spawn_native(&command, None, None) {
            Ok(Some(child)) => child,
            Ok(None) => panic!("native backend unavailable"),
            Err(error) if mode != "drop" => {
                assert!(error.to_string().contains("termination signal"), "{error}");
                break;
            }
            Err(error) => panic!("{error}"),
        };
        let mut lines = tokio::io::BufReader::new(child.take_stdout().unwrap()).lines();
        if let Some(line) = lines.next_line().await.unwrap() {
            leaders.push(line.parse::<i32>().unwrap());
        }
        if mode != "drop" || iteration % 3 == 0 {
            let status = child.reap().await.unwrap();
            if mode == "drop" {
                assert!(status.success());
            } else {
                use std::os::unix::process::ExitStatusExt;
                assert_eq!(
                    status.signal(),
                    Some(if mode == "INT" {
                        libc::SIGINT
                    } else {
                        libc::SIGTERM
                    })
                );
            }
        } else if iteration % 3 == 1 {
            assert!(tokio::time::timeout(Duration::from_millis(1), child.reap())
                .await
                .is_err());
        }
        drop(child);
    }
    leaders
}

fn assert_reaped(leaders: &[i32]) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while leaders
        .iter()
        .any(|pid| std::path::Path::new(&format!("/proc/{pid}")).exists())
    {
        assert!(
            Instant::now() < deadline,
            "native reaper left an owned leader behind"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    loop {
        // SAFETY: all workers have dropped their children; this only reaps
        // dead adopted descendants, never releases a registered group id.
        let pid = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
        if pid < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            assert_eq!(error.raw_os_error(), Some(libc::ECHILD));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "a cancelled action or descendant survived"
        );
        if pid == 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
