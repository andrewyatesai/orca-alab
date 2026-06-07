import COrca
import Foundation

/// Idiomatic Swift wrapper over the Rust headless terminal (`orca-ffi`).
///
/// This is the seam the SwiftUI macOS shell renders from: create a terminal,
/// feed it PTY output, read back grid rows / cursor. All state lives in the
/// Rust core; Swift only owns the opaque handle and a thin API.
public final class OrcaTerminal {
    private let handle: OpaquePointer

    public init(rows: Int, cols: Int) {
        // The Rust side never returns null here.
        handle = orca_terminal_new(rows, cols)
    }

    deinit {
        orca_terminal_free(handle)
    }

    /// Feed raw PTY output bytes into the grid.
    public func process(_ bytes: [UInt8]) {
        bytes.withUnsafeBufferPointer { buffer in
            orca_terminal_process(handle, buffer.baseAddress, buffer.count)
        }
    }

    public func process(_ text: String) {
        process(Array(text.utf8))
    }

    /// A row's text (trailing blanks trimmed).
    public func rowText(_ row: Int) -> String {
        guard let cString = orca_terminal_row_text(handle, row) else { return "" }
        defer { orca_string_free(cString) }
        return String(cString: cString)
    }

    /// `(row, col)` cursor position.
    public func cursor() -> (row: Int, col: Int) {
        var row = 0
        var col = 0
        orca_terminal_cursor(handle, &row, &col)
        return (row, col)
    }

    public func resize(rows: Int, cols: Int) {
        orca_terminal_resize(handle, rows, cols)
    }

    /// Grid dimensions.
    public func size() -> (rows: Int, cols: Int) {
        var rows = 0
        var cols = 0
        orca_terminal_size(handle, &rows, &cols)
        return (rows, cols)
    }

    /// The cell at `(row, col)` with its char + SGR attributes — what a SwiftUI
    /// terminal view renders from.
    public func cell(row: Int, col: Int) -> TerminalCell {
        TerminalCell(orca_terminal_cell(handle, row, col))
    }

    /// The linked Rust core's version.
    public static var coreVersion: String {
        String(cString: orca_ffi_version())
    }
}

/// A terminal color mirrored from the Rust core's `Color`.
public enum CellColor: Equatable {
    case `default`
    case indexed(UInt8)
    case rgb(UInt8, UInt8, UInt8)

    init(_ color: OrcaColor) {
        switch color.kind {
        case 1: self = .indexed(color.index)
        case 2: self = .rgb(color.r, color.g, color.b)
        default: self = .default
        }
    }
}

/// A grid cell for rendering.
public struct TerminalCell: Equatable {
    public let character: Character
    public let bold: Bool
    public let italic: Bool
    public let underline: Bool
    public let inverse: Bool
    public let foreground: CellColor
    public let background: CellColor

    init(_ cell: OrcaCell) {
        let scalar = UnicodeScalar(cell.ch) ?? UnicodeScalar(UInt8(32))
        character = Character(scalar)
        bold = cell.bold != 0
        italic = cell.italic != 0
        underline = cell.underline != 0
        inverse = cell.inverse != 0
        foreground = CellColor(cell.fg)
        background = CellColor(cell.bg)
    }
}
