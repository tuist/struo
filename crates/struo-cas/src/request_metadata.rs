//! What a Remote Execution API request says about the run and action it
//! belongs to, sent as `RequestMetadata` the way Bazel names its actions.
//!
//! The run is process-wide: the CLI names it once a run starts, with the
//! same run ID its live run events carry, so the service can join each
//! remote action to the run. The action follows the task that executes it
//! through [`with_action`]. Both are descriptive only: an execution service
//! never refuses an action for missing or unusual values.

use std::future::Future;
use std::sync::RwLock;

use prost::Message as _;

use crate::reapi;

/// gRPC metadata key the Remote Execution API defines for `RequestMetadata`.
pub const REQUEST_METADATA_KEY: &str = "build.bazel.remote.execution.v2.requestmetadata-bin";
const TOOL_NAME: &str = "struo";

/// How a remote action is labelled: its kind and the target it belongs to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionLabel {
    /// The action's kind, such as `elixir-compile`.
    pub mnemonic: String,
    /// The target the action belongs to.
    pub target: String,
}

static INVOCATION: RwLock<Option<Invocation>> = RwLock::new(None);

#[derive(Debug, Clone)]
struct Invocation {
    id: String,
    tool_version: String,
}

tokio::task_local! {
    static ACTION: ActionLabel;
}

/// Name the run remote actions of this process belong to, replacing any
/// earlier one.
pub fn set_invocation(id: impl Into<String>, tool_version: impl Into<String>) {
    let invocation = Invocation {
        id: id.into(),
        tool_version: tool_version.into(),
    };
    *INVOCATION
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(invocation);
}

/// Run `future` with remote actions it executes labelled `label`.
pub async fn with_action<F: Future>(label: ActionLabel, future: F) -> F::Output {
    ACTION.scope(label, future).await
}

/// The metadata for a request made now, or `None` outside any run.
pub(crate) fn current() -> Option<reapi::RequestMetadata> {
    let invocation = INVOCATION
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()?;
    let action = ACTION.try_with(Clone::clone).unwrap_or_default();
    Some(reapi::RequestMetadata {
        tool_details: Some(reapi::ToolDetails {
            tool_name: TOOL_NAME.to_string(),
            tool_version: invocation.tool_version,
        }),
        action_id: String::new(),
        tool_invocation_id: invocation.id,
        correlated_invocations_id: String::new(),
        action_mnemonic: action.mnemonic,
        target_id: action.target,
        configuration_id: String::new(),
    })
}

/// Attach the current metadata, if any, to an outgoing request.
pub(crate) fn attach<T>(request: &mut tonic::Request<T>) {
    if let Some(metadata) = current() {
        let value = tonic::metadata::MetadataValue::from_bytes(&metadata.encode_to_vec());
        request
            .metadata_mut()
            .insert_bin(REQUEST_METADATA_KEY, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn requests_name_the_run_and_the_action_in_scope() {
        set_invocation("run-0193", "2.20.0");
        let label = ActionLabel {
            mnemonic: "elixir-compile".into(),
            target: "atlas/mix_application_test".into(),
        };
        let mut request = tonic::Request::new(());
        with_action(label, async { attach(&mut request) }).await;

        let bytes = request
            .metadata()
            .get_bin(REQUEST_METADATA_KEY)
            .expect("metadata is attached")
            .to_bytes()
            .expect("valid binary metadata");
        let decoded = reapi::RequestMetadata::decode(bytes).expect("decodes");
        assert_eq!(decoded.tool_invocation_id, "run-0193");
        assert_eq!(decoded.action_mnemonic, "elixir-compile");
        assert_eq!(decoded.target_id, "atlas/mix_application_test");
        let tool = decoded.tool_details.expect("tool details");
        assert_eq!(
            (tool.tool_name.as_str(), tool.tool_version.as_str()),
            ("struo", "2.20.0")
        );

        // Outside an action's scope the run is still named.
        let mut request = tonic::Request::new(());
        attach(&mut request);
        let bytes = request
            .metadata()
            .get_bin(REQUEST_METADATA_KEY)
            .unwrap()
            .to_bytes()
            .unwrap();
        let decoded = reapi::RequestMetadata::decode(bytes).unwrap();
        assert_eq!(decoded.tool_invocation_id, "run-0193");
        assert!(decoded.action_mnemonic.is_empty());
    }
}
