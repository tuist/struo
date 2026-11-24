// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "CoverageFixture",
    platforms: [.macOS(.v14)],
    products: [.library(name: "Product", targets: ["Product"])],
    targets: [
        .target(name: "Product", path: "Product"),
        .testTarget(name: "SwiftTests", dependencies: ["Product"], path: ".", sources: ["SwiftTests.swift"]),
        .testTarget(name: "XCTestTests", dependencies: ["Product"], path: ".", sources: ["XCTestTests.swift"]),
    ]
)
