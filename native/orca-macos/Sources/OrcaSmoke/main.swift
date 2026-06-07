import Foundation
import OrcaKit

// Drives the Rust core through the Swift wrapper exactly as the macOS shell
// will — proving the Swift↔OrcaKit↔orca-ffi↔(vendored vte) seam links and
// round-trips. Exits non-zero on any mismatch.

func check(_ condition: Bool, _ message: String) {
    if !condition {
        FileHandle.standardError.write(Data("FAIL: \(message)\n".utf8))
        exit(1)
    }
}

let terminal = OrcaTerminal(rows: 24, cols: 80)
terminal.process("hi\r\nthere")
check(terminal.rowText(0) == "hi", "row 0 should be 'hi'")
check(terminal.rowText(1) == "there", "row 1 should be 'there'")
let cursor = terminal.cursor()
check(cursor.row == 1 && cursor.col == 5, "cursor should be (1, 5), got \(cursor)")

let resized = OrcaTerminal(rows: 4, cols: 10)
resized.process("\u{1b}]7;file:///srv/app\u{07}top\r\nbot")
resized.resize(rows: 8, cols: 20)
check(resized.rowText(0) == "top", "resized row 0 should be 'top'")
check(resized.rowText(1) == "bot", "resized row 1 should be 'bot'")

check(!OrcaTerminal.coreVersion.isEmpty, "core version should be non-empty")

// Per-cell rendering data (char + SGR attributes + truecolor) through the ABI.
let styled = OrcaTerminal(rows: 2, cols: 10)
styled.process("\u{1b}[1;38;2;10;20;30mZ")
let cell = styled.cell(row: 0, col: 0)
check(cell.character == "Z", "styled cell char should be 'Z'")
check(cell.bold, "styled cell should be bold")
check(cell.foreground == .rgb(10, 20, 30), "styled cell fg should be truecolor (10,20,30)")
check(styled.size() == (rows: 2, cols: 10), "size should round-trip")

// Live session: spawn a real shell command in a PTY through the FFI and read
// its streamed output back — the full Swift → C ABI → orca-session → PTY →
// terminal → Swift path, running.
guard let session = OrcaSession(program: "/bin/sh", args: ["-c", "printf live-from-swift"], rows: 24, cols: 80) else {
    FileHandle.standardError.write(Data("FAIL: session spawn returned nil\n".utf8))
    exit(1)
}
session.wait()
let liveRow = session.rowText(0)
check(liveRow.contains("live-from-swift"), "live session row 0 should contain output, got \(liveRow)")

print("OK — Swift shell drove the Rust core (grid, cursor, OSC-7 cwd, resize, per-cell SGR/truecolor) AND a live PTY session; core v\(OrcaTerminal.coreVersion)")
