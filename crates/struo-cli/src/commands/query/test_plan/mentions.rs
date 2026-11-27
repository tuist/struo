use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use aho_corasick::AhoCorasick;

use super::inputs::TargetInputs;

/// Files larger than this are not searched; they count as naming every path,
/// so a test that might read one is still selected.
const SEARCH_LIMIT_BYTES: u64 = 16 * 1024 * 1024;

/// Quoted text longer than this is not read as a directory name.
const LITERAL_LIMIT_BYTES: usize = 512;

/// The targets whose `_affected_mentions` files name each of `paths`.
///
/// A file names a path when it contains the path relative to the workspace,
/// to the directory the target's mention globs start from, or to a directory
/// above that one, as `../` paths reach it. It also names a path when a quoted
/// string in it names a directory the path lies in, as `"docs/reference"`,
/// `"./migrations"`, `"$ROOT/assets"`, or `"fixtures/**/*.json"` do. A bare
/// word such as `"dev"` names no directory, since it is as likely to be any
/// other string. Each directory is walked once and each file read once,
/// however many paths and targets ask about it.
pub(super) fn mention_owners<'a>(
    workspace: &Path,
    targets: &'a BTreeMap<String, TargetInputs>,
    paths: &[String],
) -> BTreeMap<String, BTreeSet<&'a str>> {
    let mut owners = BTreeMap::<String, BTreeSet<&str>>::new();
    if paths.is_empty() {
        return owners;
    }
    let mut walks = BTreeMap::<(String, Vec<String>), Vec<(&str, &TargetInputs)>>::new();
    for (id, inputs) in targets.iter().filter(|(_, inputs)| inputs.has_mentions()) {
        walks
            .entry(inputs.walk_key())
            .or_default()
            .push((id.as_str(), inputs));
    }
    for ((root, _), group) in walks {
        let needles = Needles::new(&root, paths);
        let Ok(automaton) = AhoCorasick::new(&needles.files) else {
            continue;
        };
        for file in searched_files(workspace, &root, group[0].1) {
            let searchers = group
                .iter()
                .filter(|(_, inputs)| inputs.searches(&file))
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            if searchers.is_empty() {
                continue;
            }
            let named = readable(&workspace.join(&file)).map(|text| {
                let files = automaton
                    .find_overlapping_iter(&text)
                    .filter(|found| whole_name(&text, found.start(), found.end()))
                    .map(|found| found.pattern().as_usize())
                    .collect::<BTreeSet<_>>();
                (files, quoted_directories(&text))
            });
            for (path, (file_needles, directory_needles)) in paths.iter().zip(&needles.by_path) {
                let is_named = named.as_ref().is_none_or(|(files, directories)| {
                    file_needles.iter().any(|needle| files.contains(needle))
                        || directory_needles
                            .iter()
                            .any(|directory| directories.contains(directory))
                });
                if is_named {
                    owners
                        .entry(path.clone())
                        .or_default()
                        .extend(searchers.iter().copied());
                }
            }
        }
    }
    owners
}

/// What a search under one directory looks for: each path, and each
/// directory it lies in, relative to the workspace and to every directory from
/// the search root up.
struct Needles {
    files: Vec<String>,
    /// Per path, the indices of its `files` needles and its directory names.
    by_path: Vec<(Vec<usize>, Vec<String>)>,
}

impl Needles {
    fn new(root: &str, paths: &[String]) -> Self {
        let mut bases = vec![String::new()];
        let mut base = String::new();
        for part in root.split('/').filter(|part| !part.is_empty()) {
            if !base.is_empty() {
                base.push('/');
            }
            base.push_str(part);
            bases.push(base.clone());
        }
        let relative = |path: &str| {
            bases
                .iter()
                .filter_map(|base| {
                    if base.is_empty() {
                        Some(path.to_string())
                    } else {
                        path.strip_prefix(base.as_str())
                            .and_then(|rest| rest.strip_prefix('/'))
                            .map(str::to_string)
                    }
                })
                .filter(|relative| !relative.is_empty())
                .collect::<BTreeSet<_>>()
        };
        let mut files = Vec::<String>::new();
        let by_path = paths
            .iter()
            .map(|path| {
                let file_needles = relative(path)
                    .into_iter()
                    .map(|needle| {
                        files
                            .iter()
                            .position(|known| *known == needle)
                            .unwrap_or_else(|| {
                                files.push(needle);
                                files.len() - 1
                            })
                    })
                    .collect();
                let directory_needles = path
                    .match_indices('/')
                    .flat_map(|(end, _)| relative(&path[..end]))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                (file_needles, directory_needles)
            })
            .collect();
        Self { files, by_path }
    }
}

/// Files under `root`, without entering the trees `inputs` skips or Struo's
/// own state.
fn searched_files(workspace: &Path, root: &str, inputs: &TargetInputs) -> Vec<String> {
    let relative = |path: &Path| {
        path.strip_prefix(workspace).ok().map(|path| {
            path.components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
    };
    walkdir::WalkDir::new(workspace.join(root))
        .into_iter()
        .filter_entry(|entry| {
            !entry.file_type().is_dir()
                || relative(entry.path()).is_some_and(|directory| {
                    directory != ".struo" && !inputs.excludes_tree(&directory)
                })
        })
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| relative(entry.path()))
        .collect()
}

/// The file's bytes, or `None` when it cannot be searched.
fn readable(path: &Path) -> Option<Vec<u8>> {
    let size = std::fs::metadata(path).ok()?.len();
    (size <= SEARCH_LIMIT_BYTES)
        .then(|| std::fs::read(path).ok())
        .flatten()
}

fn part_of_name(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
}

/// Whether `text[start..end]` is a whole path, not part of a longer name such
/// as `README.md` inside `OLD_README.md` or `README.md.bak`.
fn whole_name(text: &[u8], start: usize, end: usize) -> bool {
    (start == 0 || !part_of_name(text[start - 1]))
        && text.get(end).is_none_or(|&byte| !part_of_name(byte))
}

/// Directories the quoted strings in `text` name. A string names one when,
/// after leading `./`, `../`, or `$NAME/` parts, it is a directory path
/// followed by nothing, a `/`, or a glob, and it is more than a bare word.
fn quoted_directories(text: &[u8]) -> BTreeSet<String> {
    let mut directories = BTreeSet::new();
    for (start, &quote) in text.iter().enumerate() {
        if quote != b'"' && quote != b'\'' {
            continue;
        }
        let body = &text[start + 1..text.len().min(start + 1 + LITERAL_LIMIT_BYTES)];
        let Some(length) = body.iter().position(|&byte| byte == quote || byte == b'\n') else {
            continue;
        };
        if body[length] != quote {
            continue;
        }
        let Ok(literal) = std::str::from_utf8(&body[..length]) else {
            continue;
        };
        // A suffix joined onto another directory, as in `join(dir, "/../testdata")`,
        // is relative however it is written; no absolute path starts this way.
        let mut rest = literal
            .strip_prefix('/')
            .filter(|joined| joined.starts_with("./") || joined.starts_with("../"))
            .unwrap_or(literal);
        loop {
            if let Some(next) = rest.strip_prefix("./").or_else(|| rest.strip_prefix("../")) {
                rest = next;
            } else if let Some((variable, next)) =
                rest.strip_prefix('$').and_then(|r| r.split_once('/'))
            {
                let name = variable.trim_start_matches('{').trim_end_matches('}');
                if name.is_empty()
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    break;
                }
                rest = next;
            } else {
                break;
            }
        }
        let parts = rest.split('/').collect::<Vec<_>>();
        let glob = parts.iter().position(|part| part.contains(['*', '?', '[']));
        let directory = match glob {
            Some(index) => parts[..index].join("/"),
            None => rest.trim_end_matches('/').to_string(),
        };
        let plain = |part: &str| !part.is_empty() && part != "." && part != "..";
        if literal.contains('/') && directory.split('/').all(plain) {
            directories.insert(directory);
        }
    }
    directories
}

#[cfg(test)]
mod tests {
    use super::{quoted_directories, whole_name};

    fn names(text: &str, needle: &str) -> bool {
        text.match_indices(needle)
            .any(|(start, _)| whole_name(text.as_bytes(), start, start + needle.len()))
    }

    #[test]
    fn a_mention_is_a_whole_path() {
        assert!(names("include_str!(\"../dev/a.toml\")", "dev/a.toml"));
        assert!(names("read(\"README.md\")", "README.md"));
        assert!(!names("read(\"OLD_README.md\")", "README.md"));
        assert!(!names("read(\"README.md.bak\")", "README.md"));
        assert!(!names("read(\"xdev/a.toml\")", "dev/a.toml"));
    }

    #[test]
    fn quoted_strings_name_directories_only_when_written_as_paths() {
        let named = quoted_directories(
            br#"read_dir("docs/reference"); migrate!("./migrations"); folder("$ROOT/assets/");
            glob("fixtures/**/*.json"); join("dev"); open("/tmp/dev"); "x/a.toml";
            join(here, "/../testdata"); join(here, "/./golden/")"#,
        );
        for directory in [
            "docs/reference",
            "migrations",
            "assets",
            "fixtures",
            "testdata",
            "golden",
        ] {
            assert!(named.contains(directory), "{directory}: {named:?}");
        }
        for unnamed in ["dev", "tmp", "tmp/dev"] {
            assert!(!named.contains(unnamed), "{unnamed}: {named:?}");
        }
    }
}
