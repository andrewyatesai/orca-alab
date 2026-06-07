// swift-tools-version:5.9
import Foundation
import PackageDescription

// The thin macOS shell for the native-Rust Orca: a SwiftUI app links the Rust
// core through its C ABI (orca-ffi → liborca_ffi). This package is the
// Swift↔Rust seam — OrcaKit wraps the C API in idiomatic Swift; a SwiftUI app
// target sits on top of OrcaKit.
//
// Build the core first:  (cd ../../rust && cargo build -p orca-ffi)
// Then:                  swift test
let packageDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path
let rustTarget = "\(packageDir)/../../rust/target/debug"

let linkSettings: [LinkerSetting] = [
    .unsafeFlags([
        "-L\(rustTarget)",
        "-lorca_ffi",
        // Rust std on macOS pulls these in.
        "-framework", "CoreFoundation",
        "-framework", "Security",
    ])
]

let package = Package(
    name: "orca-macos",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "OrcaKit", targets: ["OrcaKit"]),
        .library(name: "OrcaUI", targets: ["OrcaUI"]),
        .executable(name: "orca-app", targets: ["OrcaApp"]),
        .executable(name: "orca-smoke", targets: ["OrcaSmoke"]),
    ],
    targets: [
        // C ABI module map over orca-ffi's header.
        .systemLibrary(name: "COrca", path: "Sources/COrca"),
        // Idiomatic Swift wrapper; links the vendored Rust static lib.
        .target(name: "OrcaKit", dependencies: ["COrca"], linkerSettings: linkSettings),
        // SwiftUI views rendering OrcaKit's terminal cells — the macOS UI.
        .target(name: "OrcaUI", dependencies: ["OrcaKit"]),
        // The windowed macOS app: spawns $SHELL in a live PTY session, renders
        // it, forwards key input. The thin Swift shell over the Rust core.
        .executableTarget(name: "OrcaApp", dependencies: ["OrcaKit", "OrcaUI"], linkerSettings: linkSettings),
        // Smoke executable verifying the Swift↔Rust seam (no XCTest, which the
        // Command Line Tools toolchain here lacks). `swift run orca-smoke`.
        .executableTarget(name: "OrcaSmoke", dependencies: ["OrcaKit"], linkerSettings: linkSettings),
    ]
)
