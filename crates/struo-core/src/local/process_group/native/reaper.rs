use std::io;
use std::process::ExitStatus;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use super::super::registry::SLOTS;
use super::sys;

// Reserve cleanup capacity before spawning. Only dropped owners publish a
// positive pid, after the group has been killed and unregistered. One thread
// reaps those pids even after the Tokio runtime has shut down.
static PIDS: Mutex<[libc::pid_t; SLOTS]> = Mutex::new([0; SLOTS]);
static WAKE: Condvar = Condvar::new();
static STARTED: OnceLock<io::Result<()>> = OnceLock::new();

pub(super) struct Owner {
    slot: usize,
    pid: Option<libc::pid_t>,
}

impl Owner {
    pub(super) fn reserve() -> io::Result<Self> {
        STARTED
            .get_or_init(|| {
                std::thread::Builder::new()
                    .name("struo-child-reaper".into())
                    .spawn(reap_orphans)
                    .map(drop)
            })
            .as_ref()
            .map_err(|error| io::Error::other(error.to_string()))?;
        let mut slots = PIDS.lock().unwrap();
        let slot = slots
            .iter()
            .position(|pid| *pid == 0)
            .ok_or_else(|| io::Error::other("native child cleanup capacity exhausted"))?;
        slots[slot] = -1;
        Ok(Self { slot, pid: None })
    }

    pub(super) fn assign(&mut self, pid: libc::pid_t) {
        self.pid = Some(pid);
    }

    pub(super) fn reap(&mut self) -> io::Result<ExitStatus> {
        let result = sys::try_wait(self.pid.expect("only reap a live owner"));
        match result {
            Ok(Some(status)) => {
                self.pid = None;
                Ok(status)
            }
            Ok(None) => Err(io::ErrorKind::WouldBlock.into()),
            Err(error) => {
                if error.raw_os_error() == Some(libc::ECHILD) {
                    self.pid = None;
                }
                Err(error)
            }
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if let Some(pid) = self.pid {
            sys::kill(pid);
        }
        let mut slots = PIDS.lock().unwrap();
        slots[self.slot] = self.pid.unwrap_or(0);
        if self.pid.is_some() {
            WAKE.notify_one();
        }
    }
}

fn reap_orphans() {
    let mut slots = PIDS.lock().unwrap();
    loop {
        let mut pending = false;
        for pid in slots.iter_mut().filter(|pid| **pid > 0) {
            match sys::try_wait(*pid) {
                Ok(Some(_)) => *pid = 0,
                Err(error) if error.raw_os_error() == Some(libc::ECHILD) => *pid = 0,
                _ => pending = true,
            }
        }
        slots = if pending {
            WAKE.wait_timeout(slots, Duration::from_millis(100))
                .unwrap()
                .0
        } else {
            WAKE.wait(slots).unwrap()
        };
    }
}
