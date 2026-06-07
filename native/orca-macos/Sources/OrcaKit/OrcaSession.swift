import COrca
import Foundation

/// A live terminal session: spawns a command in a PTY whose output streams into
/// a Rust headless terminal. The macOS app creates one of these per terminal
/// pane and renders its grid through `cell(row:col:)` / `rowText(_:)`.
public final class OrcaSession {
    private let handle: OpaquePointer

    /// Spawn `program` with `args` in a `rows`×`cols` PTY. Returns nil on failure.
    public init?(program: String, args: [String], rows: Int, cols: Int) {
        let cStrings = args.map { strdup($0) }
        defer { cStrings.forEach { free($0) } }
        let argPointers: [UnsafePointer<CChar>?] = cStrings.map { mutable in
            mutable.map { UnsafePointer<CChar>($0) }
        }

        let spawned: OpaquePointer? = program.withCString { programPointer in
            argPointers.withUnsafeBufferPointer { buffer in
                orca_session_spawn(programPointer, buffer.baseAddress, args.count, rows, cols)
            }
        }
        guard let spawned else { return nil }
        handle = spawned
    }

    deinit { orca_session_free(handle) }

    /// Wait for the child to exit and all output to drain (test/headless use).
    public func wait() { orca_session_wait(handle) }

    public func write(_ bytes: [UInt8]) {
        bytes.withUnsafeBufferPointer { orca_session_write(handle, $0.baseAddress, $0.count) }
    }

    public func write(_ text: String) { write(Array(text.utf8)) }

    public func resize(rows: Int, cols: Int) { orca_session_resize(handle, rows, cols) }

    public func size() -> (rows: Int, cols: Int) {
        var rows = 0
        var cols = 0
        orca_session_size(handle, &rows, &cols)
        return (rows, cols)
    }

    public func cursor() -> (row: Int, col: Int) {
        var row = 0
        var col = 0
        orca_session_cursor(handle, &row, &col)
        return (row, col)
    }

    public func rowText(_ row: Int) -> String {
        guard let cString = orca_session_row_text(handle, row) else { return "" }
        defer { orca_string_free(cString) }
        return String(cString: cString)
    }

    public func cell(row: Int, col: Int) -> TerminalCell {
        TerminalCell(orca_session_cell(handle, row, col))
    }
}
