use std::path::Path;

use crate::remote::PreparedCommand;
use crate::{resolve_execution_argv, Error, Result};

pub(super) use crate::remote::path::output_roots;

pub(super) fn arguments(command: &PreparedCommand<'_>, root: &Path) -> Result<Vec<String>> {
    let arguments = resolve_execution_argv(command.argv, root);
    if arguments.is_empty() {
        return Err(Error::EmptyArgv);
    }
    if command.cwd.is_none()
        && command.redirect.stdout.is_none()
        && command.redirect.stderr.is_none()
    {
        return Ok(arguments);
    }

    // Keep REAPI outputs relative to the input root. Only the child changes
    // directory; all paths and arguments are positional shell parameters.
    let mut script = String::from("set -e; ");
    let mut parameters = Vec::new();
    if let Some(path) = command.redirect.stdout {
        parameters.push(path.resolve(root).to_string_lossy().into_owned());
        script.push_str("exec >\"$1\"; shift; ");
    }
    if let Some(path) = command.redirect.stderr {
        if command.redirect.stdout == Some(path) {
            script.push_str("exec 2>&1; ");
        } else {
            parameters.push(path.resolve(root).to_string_lossy().into_owned());
            script.push_str("exec 2>\"$1\"; shift; ");
        }
    }
    if let Some(cwd) = command.cwd {
        parameters.push(cwd.resolve(root).to_string_lossy().into_owned());
        script.push_str("cd -- \"$1\"; shift; ");
    }
    script.push_str("exec \"$@\"");
    let mut wrapped = vec![
        "/bin/sh".into(),
        "-c".into(),
        script,
        "struo-command".into(),
    ];
    wrapped.extend(parameters);
    wrapped.extend(arguments);
    Ok(wrapped)
}
