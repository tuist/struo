use std::collections::{BTreeMap, BTreeSet};

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

use crate::WorkspacePath;

#[derive(Default)]
pub(super) struct Counters {
    files: BTreeMap<String, BTreeMap<u64, u64>>,
    paths: BTreeMap<String, usize>,
    scopes: Vec<Value>,
    collector: Option<[Value; 3]>,
    universe: Option<BTreeMap<String, BTreeSet<u64>>>,
}

impl Counters {
    pub fn observe(&mut self, target: &str, unit: &str, coverage: &Value) -> Result<bool> {
        if coverage["partial"] != false {
            return Ok(false);
        }
        let identity = ["tool", "tool_version", "platform"].map(|field| coverage[field].clone());
        ensure!(
            identity
                .iter()
                .all(|value| value.as_str().is_some_and(|value| !value.is_empty())),
            "incomplete collector identity"
        );
        if let Some(previous) = &self.collector {
            ensure!(*previous == identity, "incompatible coverage collectors");
        }
        self.collector = Some(identity);
        let mut universe = BTreeMap::new();
        let mut covered = Vec::new();
        for file in coverage["files"]
            .as_array()
            .context("missing instrumented files")?
        {
            let path = file["path"].as_str().context("missing instrumented path")?;
            WorkspacePath::try_from(path)?;
            let numbers = file["line_numbers"]
                .as_array()
                .context("missing instrumented lines")?;
            let counts = file["execution_counts"]
                .as_array()
                .context("missing execution counts")?;
            ensure!(
                numbers.len() == counts.len() && !numbers.is_empty(),
                "incomplete instrumented counters"
            );
            let mut lines = BTreeSet::new();
            let aggregate = self.files.entry(path.into()).or_default();
            let mut hit = false;
            for (line, count) in numbers.iter().zip(counts) {
                let line = line
                    .as_u64()
                    .filter(|line| *line > 0)
                    .context("invalid instrumented line")?;
                let count = count.as_u64().context("invalid execution count")?;
                ensure!(lines.insert(line), "duplicate instrumented line");
                let total = aggregate.entry(line).or_default();
                *total = total
                    .checked_add(count)
                    .context("coverage count overflow")?;
                hit |= count > 0;
            }
            ensure!(
                universe.insert(path.to_owned(), lines).is_none(),
                "duplicate instrumented file"
            );
            if hit {
                let next = self.paths.len();
                covered.push(*self.paths.entry(path.to_owned()).or_insert(next));
            }
        }
        let complete = !universe.is_empty()
            && self
                .universe
                .as_ref()
                .is_none_or(|previous| *previous == universe);
        self.universe = Some(universe);
        self.scopes.push(
            json!({"kind":"test", "module":target, "suite":"", "name":unit, "files":covered}),
        );
        Ok(complete)
    }

    pub fn finish(self, target: &str, complete: bool, result: &mut Value) {
        let mut paths = vec![String::new(); self.paths.len()];
        for (path, index) in self.paths {
            paths[index] = path;
        }
        result["coverage_evidence"] = json!({"status":if complete {"collected"} else {"failed"}, "paths":paths,
            "scopes":if complete {self.scopes} else {Vec::new()}});
        if let Some([tool, version, platform]) = self.collector {
            let files = self
                .files
                .into_iter()
                .map(|(path, lines)| {
                    json!({
                        "path":path, "targets":[target], "executable_lines":lines.len(),
                        "covered_lines":lines.values().filter(|count| **count > 0).count(),
                        "line_numbers":lines.keys().collect::<Vec<_>>(),
                        "execution_counts":lines.values().collect::<Vec<_>>(), "functions":[],
                    })
                })
                .collect::<Vec<_>>();
            result["coverage"] = json!({"tool":tool,"tool_version":version,"platform":platform,"partial":!complete,"files":files});
        }
    }
}
