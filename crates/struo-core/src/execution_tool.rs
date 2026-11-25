//! Explicit tool bindings between analysis and a prepared execution host.

use serde::{Deserialize, Serialize};

const PREFIX: &str = "{{struo.tool.";

/// A discovery command returning an absolute path on the execution host.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionTool {
    /// Analysis-host executable, used for local execution.
    pub path: String,
    /// Direct argv producing one absolute path in the prepared environment.
    pub resolve: Vec<String>,
    /// Relative executable path below the discovery result, or empty.
    pub suffix: String,
    /// SHA-256 of the analysis-host executable, checked before remote use.
    pub sha256: String,
}

impl ExecutionTool {
    /// Encode a binding as a command value without shell or path separators.
    pub fn marker(&self) -> String {
        format!(
            "{PREFIX}{}}}}}",
            hex::encode(serde_json::to_vec(self).unwrap())
        )
    }

    /// Find bindings, including ones embedded in shell arguments or env values.
    pub fn find(value: &str) -> std::result::Result<Vec<(String, Self)>, String> {
        let mut found = Vec::new();
        let mut rest = value;
        while let Some(start) = rest.find(PREFIX) {
            rest = &rest[start..];
            let end = rest
                .find("}}")
                .ok_or("unterminated execution tool binding")?
                + 2;
            if end > 32 * 1024 {
                return Err("execution tool binding exceeds 32 KiB".into());
            }
            let bytes = hex::decode(&rest[PREFIX.len()..end - 2])
                .map_err(|_| "invalid execution tool encoding")?;
            let tool: Self =
                serde_json::from_slice(&bytes).map_err(|_| "invalid execution tool binding")?;
            if tool.resolve.first().is_none_or(String::is_empty)
                || tool.resolve.iter().any(|arg| arg.contains('\0'))
                || !std::path::Path::new(&tool.path).is_absolute()
                || tool.sha256.len() != 64
                || !tool.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || tool.suffix.split('/').any(|part| part == "..")
                || tool.suffix.starts_with('/')
                || tool.suffix.contains(['\\', '\0'])
            {
                return Err("invalid execution tool path, resolver or digest".into());
            }
            found.push((rest[..end].into(), tool));
            rest = &rest[end..];
        }
        Ok(found)
    }
}

pub(crate) fn resolve_local(value: &str) -> String {
    ExecutionTool::find(value).map_or_else(
        |_| value.to_string(),
        |tools| {
            tools
                .into_iter()
                .fold(value.to_string(), |value, (marker, tool)| {
                    value.replace(&marker, &tool.path)
                })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_round_trip_inside_command_and_environment_values() {
        let tool = ExecutionTool {
            path: "/analysis/tools/bin/compiler".into(),
            resolve: vec!["compiler".into(), "--root".into()],
            suffix: "bin/compiler".into(),
            sha256: "ab".repeat(32),
        };
        let marker = tool.marker();
        let value = format!("exec '{marker}' --driver={marker}");
        assert_eq!(ExecutionTool::find(&value).unwrap().len(), 2);
        assert_eq!(
            resolve_local(&value),
            "exec '/analysis/tools/bin/compiler' --driver=/analysis/tools/bin/compiler"
        );
        assert!(ExecutionTool::find("{{struo.tool.bad}}").is_err());
        assert!(ExecutionTool::find("{{struo.tool.unclosed").is_err());
        assert!(ExecutionTool::find(
            &ExecutionTool {
                suffix: "../escape".into(),
                ..tool
            }
            .marker()
        )
        .is_err());
    }
}
