use std::collections::BTreeMap;

use starlark::environment::GlobalsBuilder;
use starlark::starlark_module;
use starlark::values::{none::NoneType, Value};

use crate::analysis::presentation::record_action;
use crate::analysis::store::{analysis_active, DeclaredAction, DeclaredActionOperation};
use crate::analysis::values::unpack_string_list;

#[starlark_module]
pub(super) fn test_action_globals(builder: &mut GlobalsBuilder) {
    fn merge_test_coverage<'v>(
        target: &str,
        inventory: &str,
        results: Value<'v>,
        output: &str,
        #[starlark(require = named, default = false)] partial: bool,
    ) -> anyhow::Result<NoneType> {
        if !analysis_active() {
            return Ok(NoneType);
        }
        let results = unpack_string_list(results, "results")?;
        let mut inputs = results.clone();
        inputs.push(inventory.into());
        inputs.sort();
        inputs.dedup();
        record_action(DeclaredAction {
            operation: Some(DeclaredActionOperation::MergeTestCoverage {
                target: target.into(),
                inventory: inventory.into(),
                results,
                output: output.into(),
                partial,
            }),
            argv: Vec::new().into(),
            arg_files: Vec::new(),
            inputs: inputs.into(),
            outputs: vec![output.into()],
            stdout: None,
            stderr: None,
            clean_paths: Vec::new(),
            create_dirs: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            sandbox: None,
            network: None,
            success_exit_codes: vec![0],
            cacheable: false,
            inherit_parent_env: false,
            depends_on_prior_actions: false,
            intermediate: false,
            toolchain_identity: None,
            identifier: Some(format!("merge_test_coverage:{target}")),
            display_name: Some(format!("Collect test coverage · {target}")),
            source_files: Vec::new(),
            presentation: None,
            history: None,
            resources: struo_core::ResourceRequest::default(),
        });
        Ok(NoneType)
    }
}
