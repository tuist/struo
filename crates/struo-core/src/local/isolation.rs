//! Builds the command for a local action, wrapping it in the platform's
//! network isolation when the action declared `network = "deny"`.
//!
//! Denial fails closed: a platform or architecture without an enforcement
//! mechanism refuses to run the action instead of running it with the
//! network reachable.

use tokio::process::Command;

use crate::{NetworkPolicy, Result};

/// The Seatbelt rules for a denied action. Every network operation is
/// denied, loopback included, and then `AF_UNIX` sockets are allowed back so
/// local inter-process communication keeps working. The system DNS resolver
/// is reached over a Unix socket and would resolve names on the action's
/// behalf, so it stays denied.
#[cfg(target_os = "macos")]
pub(crate) const DENY_NETWORK_RULES: &str = concat!(
    " (deny network*)",
    " (allow network* (local unix-socket))",
    " (allow network* (remote unix-socket))",
    " (deny network-outbound (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\")))",
);

/// The command that runs `program` under `network`.
pub(super) fn command(network: NetworkPolicy, program: &str, args: &[String]) -> Result<Command> {
    if !network.is_denied() {
        let mut command = Command::new(program);
        command.args(args);
        return Ok(command);
    }
    deny_network(program, args)
}

/// The command that runs `program` under the access trace helper. On Linux
/// the helper runs inside the network isolation and its tracees inherit it.
/// On macOS Seatbelt profiles do not nest, so the helper merges the network
/// denial into its own profile instead.
pub(super) fn traced_command(
    network: NetworkPolicy,
    helper: &crate::access_trace::Wrapper,
    program: &str,
    args: &[String],
) -> Result<Command> {
    let helper_program = helper.program.to_string_lossy().into_owned();
    let mut helper_args = vec![
        crate::access_trace::HELPER_VERB.to_string(),
        helper.spec.to_string_lossy().into_owned(),
        "--".to_string(),
        program.to_string(),
    ];
    helper_args.extend(args.iter().cloned());
    if cfg!(target_os = "macos") {
        let mut command = Command::new(helper_program);
        command.args(helper_args);
        return Ok(command);
    }
    command(network, &helper_program, &helper_args)
}

#[cfg(target_os = "macos")]
#[allow(clippy::unnecessary_wraps)]
fn deny_network(program: &str, args: &[String]) -> Result<Command> {
    let mut command = Command::new("/usr/bin/sandbox-exec");
    let profile = format!("(version 1) (allow default){DENY_NETWORK_RULES}");
    command.args(["-p", &profile, "--", program]);
    command.args(args);
    Ok(command)
}

#[cfg(target_os = "linux")]
fn deny_network(program: &str, args: &[String]) -> Result<Command> {
    let mut command = Command::new(program);
    command.args(args);
    match crate::network::arm(&mut command) {
        Ok(()) => Ok(command),
        Err(requirement) => Err(unenforceable(program, requirement)),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn deny_network(program: &str, _args: &[String]) -> Result<Command> {
    Err(unenforceable(program, "network denial on this platform"))
}

#[cfg(not(target_os = "macos"))]
fn unenforceable(program: &str, requirement: &'static str) -> crate::Error {
    tracing::warn!(
        program = %program,
        requirement,
        "network `deny` cannot be enforced here; refusing to run the action"
    );
    crate::Executor::Local.unsupported(requirement)
}
