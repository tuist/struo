//! Filtering path expansions through ignore files such as `.gitignore`.

use std::collections::HashMap;
use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;

/// Prefix that tags an ignore-file name among a recorded expansion's patterns.
pub(super) const IGNORE_FILE_TAG: &str = "ignore-file:";

/// Split a recorded `glob_ignoring` pattern list into its ignore-file names and
/// its glob patterns.
pub(super) fn split_ignore_files(patterns: &[String]) -> (Vec<String>, Vec<String>) {
    let mut names = Vec::new();
    let mut globs = Vec::new();
    for pattern in patterns {
        match pattern.strip_prefix(IGNORE_FILE_TAG) {
            Some(name) => names.push(name.to_string()),
            None => globs.push(pattern.clone()),
        }
    }
    (names, globs)
}

/// Keep the workspace-relative `paths` that no ignore file called one of
/// `names` excludes, by Git's rules: a directory's ignore files apply to the
/// paths beneath it, a nearer file overrides a farther one, and nothing under
/// an excluded directory is included again. An earlier name takes precedence
/// over a later one at every level, the way the `ignore` crate ranks custom
/// ignore files above `.gitignore`. Only regular files inside the workspace
/// count, so a user's global excludes cannot make two checkouts disagree.
pub(super) fn retain_unignored(
    workspace_root: &Path,
    paths: Vec<String>,
    names: &[String],
) -> Vec<String> {
    let mut rules = IgnoreRules {
        workspace_root,
        names,
        matchers: HashMap::new(),
        directories: HashMap::new(),
    };
    paths
        .into_iter()
        .filter(|path| !rules.ignored(path, false))
        .collect()
}

struct IgnoreRules<'a> {
    workspace_root: &'a Path,
    names: &'a [String],
    matchers: HashMap<(usize, String), Option<Gitignore>>,
    directories: HashMap<String, bool>,
}

impl IgnoreRules<'_> {
    fn ignored(&mut self, path: &str, is_dir: bool) -> bool {
        let parent = parent_of(path);
        if !parent.is_empty() && self.directory_ignored(parent) {
            return true;
        }
        let absolute = self.workspace_root.join(path);
        for name in 0..self.names.len() {
            let mut directory = parent;
            loop {
                if let Some(matcher) = self.matcher(name, directory) {
                    match matcher.matched(&absolute, is_dir) {
                        Match::Ignore(_) => return true,
                        Match::Whitelist(_) => return false,
                        Match::None => {}
                    }
                }
                if directory.is_empty() {
                    break;
                }
                directory = parent_of(directory);
            }
        }
        false
    }

    fn directory_ignored(&mut self, directory: &str) -> bool {
        if let Some(&known) = self.directories.get(directory) {
            return known;
        }
        let ignored = self.ignored(directory, true);
        self.directories.insert(directory.to_string(), ignored);
        ignored
    }

    fn matcher(&mut self, name: usize, directory: &str) -> Option<&Gitignore> {
        let key = (name, directory.to_string());
        if !self.matchers.contains_key(&key) {
            let root = self.workspace_root.join(directory);
            let file = root.join(&self.names[name]);
            // A symbolic link could point outside the workspace, where edits
            // go unnoticed, so only regular files count.
            let matcher = std::fs::symlink_metadata(&file)
                .is_ok_and(|metadata| metadata.is_file())
                .then(|| {
                    let mut builder = GitignoreBuilder::new(&root);
                    // Like Git, a malformed line is skipped and the rest of
                    // the file still applies.
                    let _ = builder.add(&file);
                    builder.build().ok()
                })
                .flatten();
            self.matchers.insert(key.clone(), matcher);
        }
        self.matchers[&key].as_ref()
    }
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

#[cfg(test)]
mod tests {
    use super::retain_unignored;

    fn kept(files: &[(&str, &str)], paths: &[&str]) -> Vec<String> {
        let workspace = tempfile::TempDir::new().unwrap();
        for (path, content) in files {
            let path = workspace.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        let names = vec![".nxignore".to_string(), ".gitignore".to_string()];
        retain_unignored(
            workspace.path(),
            paths.iter().map(ToString::to_string).collect(),
            &names,
        )
    }

    #[test]
    fn nearer_files_override_farther_ones_but_not_an_excluded_directory() {
        let files = [
            (".gitignore", "*.log\nbuild/\n"),
            ("app/.gitignore", "!keep.log\nout/*\n!out/kept.json\n"),
            ("app/.nxignore", "notes/\n"),
            ("build/.gitignore", "!*.txt\n"),
        ];
        let paths = [
            "app/a.log",
            "app/keep.log",
            "app/out/x.json",
            "app/out/kept.json",
            "app/notes/todo.md",
            "app/src/main.ts",
            "build/report.txt",
            "lib/notes/todo.md",
            "root.log",
        ];
        assert_eq!(
            kept(&files, &paths),
            [
                "app/keep.log",
                "app/out/kept.json",
                "app/src/main.ts",
                "lib/notes/todo.md"
            ]
        );
    }

    #[test]
    fn an_earlier_name_wins_over_a_nearer_file_of_a_later_name() {
        let files = [(".nxignore", "!app/x.log\n"), ("app/.gitignore", "*.log\n")];
        assert_eq!(kept(&files, &["app/x.log", "app/y.log"]), ["app/x.log"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_ignore_file_is_not_read() {
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), "*.ts\n").unwrap();
        let workspace = tempfile::TempDir::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), workspace.path().join(".gitignore")).unwrap();
        let names = vec![".gitignore".to_string()];
        assert_eq!(
            retain_unignored(workspace.path(), vec!["a.ts".to_string()], &names),
            ["a.ts"]
        );
    }

    #[test]
    fn an_anchored_rule_applies_relative_to_its_own_directory() {
        let files = [("app/.gitignore", "/gen\n")];
        assert_eq!(
            kept(&files, &["app/gen/a.ts", "app/src/gen/b.ts", "gen/c.ts"]),
            ["app/src/gen/b.ts", "gen/c.ts"]
        );
    }
}
