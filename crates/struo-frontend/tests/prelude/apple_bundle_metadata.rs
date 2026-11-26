use super::*;

// A host with Xcode 16.2, the iOS 18.2 and macOS 15.2 SDKs, on macOS build
// 24B91: the version files Xcode reads its Info.plist build keys from.
const XCODE_HOST: &str = r#"
_XCODE = "/Applications/Xcode.app/Contents/Developer"
_IOS_SDK = _XCODE + "/Platforms/iPhoneSimulator.platform/Developer/SDKs/iPhoneSimulator18.2.sdk"
_MACOS_SDK = _XCODE + "/Platforms/MacOSX.platform/Developer/SDKs/MacOSX15.2.sdk"

def _version_plist(entries):
    body = "".join(["<key>" + key + "</key><string>" + value + "</string>" for key, value in entries.items()])
    return "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>" + body + "</dict></plist>"

_HOST_FILES = {
    "/Applications/Xcode.app/Contents/version.plist": _version_plist({"CFBundleShortVersionString": "16.2", "ProductBuildVersion": "16C5032a"}),
    "/System/Library/CoreServices/SystemVersion.plist": _version_plist({"ProductVersion": "15.1", "ProductBuildVersion": "24B91"}),
    _IOS_SDK + "/SDKSettings.json": '{"CanonicalName": "iphonesimulator18.2", "Version": "18.2"}',
    _IOS_SDK + "/System/Library/CoreServices/SystemVersion.plist": _version_plist({"ProductVersion": "18.2", "ProductBuildVersion": "22C146"}),
    _MACOS_SDK + "/SDKSettings.json": '{"CanonicalName": "macosx15.2", "Version": "15.2"}',
    _MACOS_SDK + "/System/Library/CoreServices/SystemVersion.plist": _version_plist({"ProductVersion": "15.2", "ProductBuildVersion": "24C94"}),
}

def host_file_exists(path):
    return path in _HOST_FILES

def host_file_read(path):
    return _HOST_FILES[path]

def host_which(name):
    return "/usr/bin/" + name

def host_command(argv, env = None, merge_stderr = None):
    if "--find" in argv:
        return "/toolchain/" + argv[len(argv) - 1] + "\n"
    if "--show-sdk-path" in argv:
        return (_MACOS_SDK if "macosx" in argv else _IOS_SDK) + "\n"
    if "--version" in argv:
        return "Swift version test\n"
    if "-print-resource-dir" in argv:
        return "/toolchain/resource\n"
    fail("unexpected host_command: " + str(argv))
"#;

fn build_application(attrs: &str, template: Option<&str>) -> AnalysisStore {
    let workspace = TempDir::new().unwrap();
    std::fs::create_dir_all(workspace.path().join("app/Sources")).unwrap();
    std::fs::write(workspace.path().join("app/Sources/App.swift"), "").unwrap();
    let template = template.map_or_else(String::new, |contents| {
        let path = workspace.path().join("app/Info.plist");
        format!("_HOST_FILES[{:?}] = {contents:?}", path.to_str().unwrap())
    });
    let source = format!(
        r#"{prelude}
{XCODE_HOST}
{template}
_apple_application_impl({{
    "label": {{"package": "app", "name": "App", "id": "app/App"}},
    "attr": dict({{"bundle_id": "dev.struo.App", "minimum_os": "17.0", "families": ["iphone"]}}, **{attrs}),
    "deps": [],
    "srcs": ["Sources/*.swift"],
    "build_dir": ".struo/out/app/App",
    "capability": "build",
}})
result = ""
"#,
        prelude = macos_prelude_source(),
    );
    let (store, out) = with_active_store(store_for(workspace.path(), "app"), || {
        eval_prelude_source_to_repr(source)
    });
    out.unwrap();
    store
}

fn written(store: &AnalysisStore, path: &str) -> Option<String> {
    store
        .actions
        .iter()
        .find_map(|action| match &action.operation {
            Some(DeclaredActionOperation::WriteFile { path: p, bytes }) if p == path => {
                Some(String::from_utf8(bytes.clone()).unwrap())
            }
            _ => None,
        })
}

fn plist_string(plist: &str, key: &str) -> Option<String> {
    let after = &plist[plist.find(&format!("<key>{key}</key>"))?..];
    let value = &after[after.find("<string>")? + "<string>".len()..];
    Some(value[..value.find("</string>")?].to_string())
}

fn assert_xcode_build_keys(plist: &str, sdk_name: &str, sdk_version: &str, sdk_build: &str) {
    for (key, expected) in [
        ("DTSDKName", format!("{sdk_name}{sdk_version}")),
        ("DTSDKBuild", sdk_build.to_string()),
        ("DTPlatformVersion", sdk_version.to_string()),
        ("DTPlatformBuild", sdk_build.to_string()),
        (
            "DTCompiler",
            "com.apple.compilers.llvm.clang.1_0".to_string(),
        ),
        ("DTXcode", "1620".to_string()),
        ("DTXcodeBuild", "16C5032a".to_string()),
        ("BuildMachineOSBuild", "24B91".to_string()),
    ] {
        assert_eq!(
            plist_string(plist, key).as_deref(),
            Some(expected.as_str()),
            "{key} in {plist}"
        );
    }
    assert_eq!(plist.matches("<key>DTSDKName</key>").count(), 1, "{plist}");
}

#[test]
fn ios_application_bundle_matches_xcode_pkginfo_and_build_keys() {
    let store = build_application(r#"{"platform": "ios", "sdk_variant": "simulator"}"#, None);

    assert_eq!(
        written(&store, ".struo/out/app/App.app/PkgInfo").as_deref(),
        Some("APPL????")
    );
    let codesign = action_by_identifier(&store, "apple_application_codesign_App");
    assert!(codesign
        .inputs
        .contains(&".struo/out/app/App.app/PkgInfo".to_string()));
    let plist = written(&store, ".struo/out/app/App.app/Info.plist").unwrap();
    assert_xcode_build_keys(&plist, "iphonesimulator", "18.2", "22C146");
}

#[test]
fn macos_application_template_gets_xcode_build_keys_and_contents_pkginfo() {
    // Xcode overwrites build keys a template carries, so a stale copy is replaced.
    let template = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>App</string>
	<key>DTXcode</key>
	<string>0941</string>
</dict>
</plist>
"#;
    let store = build_application(
        r#"{"platform": "macos", "info_plist": "Info.plist"}"#,
        Some(template),
    );

    assert_eq!(
        written(&store, ".struo/out/app/App.app/Contents/PkgInfo").as_deref(),
        Some("APPL????")
    );
    let plist = written(&store, ".struo/out/app/App.app/Contents/Info.plist").unwrap();
    assert_xcode_build_keys(&plist, "macosx", "15.2", "24C94");
    assert_eq!(plist.matches("<key>DTXcode</key>").count(), 1, "{plist}");
}

#[test]
fn application_extension_has_no_pkginfo() {
    let store = build_application(
        r#"{"platform": "ios", "sdk_variant": "simulator", "application_extension": True}"#,
        None,
    );

    assert!(store.actions.iter().all(|action| action
        .outputs
        .iter()
        .all(|output| !output.ends_with("PkgInfo"))));
    let plist = written(&store, ".struo/out/app/App.appex/Info.plist").unwrap();
    assert_xcode_build_keys(&plist, "iphonesimulator", "18.2", "22C146");
}
