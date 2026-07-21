import XCTest

final class AgentEntrypointSourceSafetyTests: XCTestCase {
    func testAgentEntrypointDoesNotUnlinkCallerSuppliedPaths() throws {
        let source = try agentSource()

        // Why: --agent accepts caller-supplied paths; deleting them in the
        // helper can remove user files if argument validation is bypassed.
        XCTAssertFalse(source.contains("unlink(tokenPath)"))
        XCTAssertFalse(source.contains("unlink(socketPath)"))
    }

    func testWindowScreenshotsUseTheSupportedMacOS14CaptureAPI() throws {
        let source = try agentSource()

        XCTAssertTrue(source.contains("SCScreenshotManager.captureImage"))
        XCTAssertFalse(source.contains("CGWindowListCreateImage("))
    }

    func testAgentPeerAllowsTheStableDevBundleIdentity() throws {
        let source = try agentSource()

        XCTAssertTrue(source.contains("bundleId == \"com.stablyai.orca.dev\""))
    }

    private func agentSource() throws -> String {
        let testFile = URL(fileURLWithPath: #filePath)
        let packageRoot = testFile
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let mainPath = packageRoot
            .appendingPathComponent("Sources")
            .appendingPathComponent("OrcaComputerUseMacOS")
            .appendingPathComponent("main.swift")
        return try String(contentsOf: mainPath, encoding: .utf8)
    }
}
