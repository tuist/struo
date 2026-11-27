// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "ResourceFixtures",
    defaultLocalization: "en",
    platforms: [.macOS(.v13)],
    products: [.executable(name: "ResourceProbe", targets: ["ResourceProbe"])],
    targets: [
        .target(name: "ImplicitAssets", exclude: ["Excluded"]),
        .target(name: "ImplicitStrings"),
        .target(name: "ObjCResources", resources: [.process("Resources")]),
        .target(name: "DeclaredAssets", resources: [.process("Resources"), .copy("Copied")]),
        .executableTarget(name: "ResourceProbe", dependencies: ["ImplicitAssets", "ImplicitStrings", "ObjCResources", "DeclaredAssets"]),
    ]
)
