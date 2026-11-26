use std::io;
use std::process::ExitStatus;

use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};

pub(super) enum Leader {
    Tokio(Child),
    #[cfg(all(feature = "linux-posix-spawn", target_os = "linux", target_env = "gnu"))]
    Native(super::native::Child),
}

impl Leader {
    pub(super) fn take_stdin(&mut self) -> Option<ChildStdin> {
        match self {
            Self::Tokio(child) => child.stdin.take(),
            #[cfg(all(feature = "linux-posix-spawn", target_os = "linux", target_env = "gnu"))]
            Self::Native(_) => None,
        }
    }

    pub(super) fn take_stdout(&mut self) -> Option<ChildStdout> {
        match self {
            Self::Tokio(child) => child.stdout.take(),
            #[cfg(all(feature = "linux-posix-spawn", target_os = "linux", target_env = "gnu"))]
            Self::Native(child) => child.stdout.take(),
        }
    }

    pub(super) fn take_stderr(&mut self) -> Option<ChildStderr> {
        match self {
            Self::Tokio(child) => child.stderr.take(),
            #[cfg(all(feature = "linux-posix-spawn", target_os = "linux", target_env = "gnu"))]
            Self::Native(child) => child.stderr.take(),
        }
    }

    pub(super) async fn wait(&mut self) -> io::Result<ExitStatus> {
        match self {
            Self::Tokio(child) => child.wait().await,
            #[cfg(all(feature = "linux-posix-spawn", target_os = "linux", target_env = "gnu"))]
            Self::Native(child) => child.reap(),
        }
    }

    #[cfg(not(unix))]
    pub(super) fn start_kill(&mut self) -> io::Result<()> {
        let Self::Tokio(child) = self;
        child.start_kill()
    }

    #[cfg(test)]
    pub(super) fn tokio(&mut self) -> &mut Child {
        match self {
            Self::Tokio(child) => child,
            #[cfg(all(feature = "linux-posix-spawn", target_os = "linux", target_env = "gnu"))]
            Self::Native(_) => panic!("native leaders cannot be reaped through Tokio"),
        }
    }
}
