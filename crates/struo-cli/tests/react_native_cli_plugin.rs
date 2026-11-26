#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

// Mirrors the React Native 0.86 install layout: `react-native` no longer pulls
// in `@react-native-community/cli`, so `react-native/cli.js` only prints the
// missing-CLI notice, and the `bundle` and `start` commands live in
// `@react-native/community-cli-plugin`.
const INSTALL: &str = r#"const fs = require('fs');
const write = (path, contents) => {
  fs.mkdirSync(require('path').dirname(path), {recursive: true});
  fs.writeFileSync(path, contents);
};
write('node_modules/react-native/package.json', JSON.stringify({
  name: 'react-native',
  version: '0.86.0',
  dependencies: {'@react-native/community-cli-plugin': '0.86.0'},
}));
write('node_modules/react-native/cli.js', `
console.error("react-native depends on @react-native-community/cli for cli commands.");
process.exit(1);
`);
write('node_modules/@react-native/community-cli-plugin/package.json', JSON.stringify({
  name: '@react-native/community-cli-plugin',
  version: '0.86.0',
  exports: {'.': './dist/index.js', './package.json': './package.json'},
}));
write('node_modules/@react-native/community-cli-plugin/dist/index.js', `
const fs = require('fs');
const path = require('path');
const checkContext = ctx => {
  if (!fs.existsSync(path.join(ctx.reactNativePath, 'package.json'))) throw new Error('bad reactNativePath');
  if (!ctx.platforms.ios || !ctx.platforms.android) throw new Error('missing platforms');
};
exports.bundleCommand = {
  name: 'bundle',
  options: [
    {name: '--entry-file <path>'},
    {name: '--platform <string>', default: 'ios'},
    {name: '--dev [boolean]', parse: val => val !== 'false', default: true},
    {name: '--minify [boolean]', parse: val => val !== 'false'},
    {name: '--bundle-output <string>'},
    {name: '--sourcemap-output <string>'},
    {name: '--assets-dest <string>'},
    {name: '--reset-cache', default: false},
    {name: '--config <string>', parse: val => path.resolve(val)},
    {name: '--resolver-option <string...>', parse: (val, previous = []) => previous.concat([val])},
  ],
  func: async (_argv, ctx, args) => {
    checkContext(ctx);
    if (!fs.existsSync(path.resolve(ctx.root, args.entryFile))) throw new Error('bad root or entry');
    if (!fs.existsSync(args.config)) throw new Error('missing config');
    fs.writeFileSync(args.bundleOutput, JSON.stringify({
      platform: args.platform,
      dev: args.dev,
      minify: args.minify,
      resetCache: args.resetCache,
      entry: fs.readFileSync(path.resolve(ctx.root, args.entryFile), 'utf8'),
      resolverOption: args.resolverOption,
    }));
    fs.writeFileSync(args.sourcemapOutput, '{}');
  },
};
exports.startCommand = {
  name: 'start',
  options: [
    {name: '--port <number>', parse: Number},
    {name: '--host <string>', default: ''},
    {name: '--projectRoot <path>', parse: val => path.resolve(val)},
    {name: '--reset-cache, --resetCache'},
    {name: '--config <string>', parse: val => path.resolve(val)},
    {name: '--no-interactive'},
    {name: '--client-logs', default: false},
  ],
  func: async (_argv, ctx, args) => {
    checkContext(ctx);
    if (!fs.existsSync(args.config)) throw new Error('missing config');
    fs.writeFileSync(path.join(args.projectRoot, 'start.json'), JSON.stringify({
      reactNativeVersion: ctx.reactNativeVersion,
      port: args.port,
      host: args.host,
      resetCache: args.resetCache,
      interactive: args.interactive,
      clientLogs: args.clientLogs,
    }));
  },
};
`);
"#;

struct Workspace {
    root: tempfile::TempDir,
    home: tempfile::TempDir,
}

impl Workspace {
    fn new(targets: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let node = Command::new("node")
            .args(["-p", "process.execPath"])
            .output()
            .expect("Node.js required for React Native CLI plugin regression");
        assert!(node.status.success(), "{node:?}");
        let node = String::from_utf8(node.stdout).unwrap().trim().to_string();
        let manager = root.path().join("npm");
        std::fs::write(
            &manager,
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 11.0.0; else node install.js; fi\n",
        )
        .unwrap();
        std::fs::set_permissions(&manager, std::fs::Permissions::from_mode(0o755)).unwrap();
        for (path, contents) in [
            ("package.json", "{}\n"),
            (
                "package-lock.json",
                r#"{"lockfileVersion":3,"packages":{"node_modules/react-native":{"version":"0.86.0"},"node_modules/hermes-compiler":{"version":"0.14.0"}}}"#,
            ),
            ("install.js", INSTALL),
            (
                "index.js",
                "export default function App() { return null }\n",
            ),
        ] {
            std::fs::write(root.path().join(path), contents).unwrap();
        }
        std::fs::write(
            root.path().join("struo.toml"),
            format!(
                r#"
[[target]]
name = "Dependencies"
kind = "react_native_dependencies"
srcs = ["install.js"]
[target.attrs]
resolver_inputs = ["package.json", "package-lock.json"]
node = {node:?}
package_manager_executable = {manager:?}
{targets}"#,
                manager = manager.to_string_lossy(),
                targets = targets.replace("{node}", &format!("{node:?}")),
            ),
        )
        .unwrap();
        Self {
            root,
            home: tempfile::tempdir().unwrap(),
        }
    }

    fn struo(&self, args: &[&str]) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_struo"))
            .arg("-C")
            .arg(self.root.path())
            .args(["--memory-limit", "512MiB"])
            .args(args)
            .env("HOME", self.home.path())
            .env("XDG_CACHE_HOME", self.home.path().join("cache"))
            .env("XDG_CONFIG_HOME", self.home.path().join("config"))
            .env("XDG_DATA_HOME", self.home.path().join("data"))
            .env("XDG_STATE_HOME", self.home.path().join("state"))
            .env("STRUO_CACHE_PROVIDER", "local")
            .env("STRUO_CHANGE_TRACKER", "0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn read_json(&self, path: &str) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(self.root.path().join(path)).unwrap())
            .unwrap()
    }
}

#[test]
fn bundle_runs_metro_through_community_cli_plugin_without_community_cli() {
    let workspace = Workspace::new(
        r#"
[[target]]
name = "Bundle"
kind = "react_native_bundle"
srcs = ["index.js"]
deps = ["./Dependencies"]
[target.attrs]
platform = "android"
hermes = false
node = {node}
bundle_args = ["--resolver-option", "a=1", "--resolver-option", "b=2"]
"#,
    );
    workspace.struo(&["build", "Bundle"]);
    assert_eq!(
        workspace.read_json(".struo/out/Bundle/bundle/main.jsbundle"),
        serde_json::json!({
            "platform": "android",
            "dev": false,
            "minify": true,
            "resetCache": true,
            "entry": "export default function App() { return null }\n",
            "resolverOption": ["a=1", "b=2"],
        })
    );
}

#[test]
fn metro_starts_through_community_cli_plugin_without_community_cli() {
    let workspace = Workspace::new(
        r#"
[[target]]
name = "Metro"
kind = "react_native_metro"
srcs = ["index.js"]
deps = ["./Dependencies"]
[target.attrs]
port = 8099
reset_cache = true
node = {node}
"#,
    );
    workspace.struo(&["run", "Metro"]);
    assert_eq!(
        workspace.read_json("start.json"),
        serde_json::json!({
            "reactNativeVersion": "0.86.0",
            "port": 8099,
            "host": "127.0.0.1",
            "resetCache": true,
            "interactive": false,
            "clientLogs": false,
        })
    );
}
