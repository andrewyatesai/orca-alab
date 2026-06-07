import OrcaKit
import SwiftUI

/// A SwiftUI view that renders an `OrcaTerminal`'s grid — the macOS terminal
/// pane. All terminal state lives in the Rust core; this view only reads cells
/// (char + SGR attributes + color) through `OrcaKit` and draws them.
///
/// The `revision` lets the host re-render after feeding new PTY output (bump it
/// whenever `terminal.process(...)` is called).
public struct TerminalView: View {
    private let terminal: OrcaTerminal
    private let revision: Int

    public init(terminal: OrcaTerminal, revision: Int = 0) {
        self.terminal = terminal
        self.revision = revision
    }

    public var body: some View {
        let size = terminal.size()
        VStack(alignment: .leading, spacing: 0) {
            ForEach(0..<size.rows, id: \.self) { row in
                HStack(spacing: 0) {
                    ForEach(0..<size.cols, id: \.self) { col in
                        cellText(terminal.cell(row: row, col: col))
                    }
                }
            }
        }
        .font(.system(.body, design: .monospaced))
        .id(revision)
    }

    @ViewBuilder
    private func cellText(_ cell: TerminalCell) -> some View {
        terminalCellView(cell)
    }
}

/// Renders a live `OrcaSession`'s grid — the actual terminal pane backed by a
/// running PTY in the Rust core. Bump `revision` after feeding input to redraw.
public struct SessionTerminalView: View {
    private let session: OrcaSession
    private let revision: Int

    public init(session: OrcaSession, revision: Int = 0) {
        self.session = session
        self.revision = revision
    }

    public var body: some View {
        let size = session.size()
        VStack(alignment: .leading, spacing: 0) {
            ForEach(0..<size.rows, id: \.self) { row in
                HStack(spacing: 0) {
                    ForEach(0..<size.cols, id: \.self) { col in
                        terminalCellView(session.cell(row: row, col: col))
                    }
                }
            }
        }
        .font(.system(.body, design: .monospaced))
        .id(revision)
    }
}

/// Render a single terminal cell (char + SGR attributes + inverse) as styled text.
@ViewBuilder
func terminalCellView(_ cell: TerminalCell) -> some View {
    let fg = cell.inverse ? cell.background : cell.foreground
    let bg = cell.inverse ? cell.foreground : cell.background
    Text(String(cell.character))
        .fontWeight(cell.bold ? .bold : .regular)
        .italic(cell.italic)
        .underline(cell.underline)
        .foregroundColor(swiftUIColor(fg) ?? .primary)
        .background(swiftUIColor(bg) ?? .clear)
}

/// Map a core `CellColor` to a SwiftUI `Color`. `nil` = use the view default.
func swiftUIColor(_ color: CellColor) -> Color? {
    switch color {
    case .default:
        return nil
    case .indexed(let index):
        return ansiPaletteColor(index)
    case .rgb(let r, let g, let b):
        return Color(red: Double(r) / 255.0, green: Double(g) / 255.0, blue: Double(b) / 255.0)
    }
}

/// The standard 16 ANSI colors (0–7 normal, 8–15 bright); higher 256-palette
/// indices fall back to a derived gray for now.
func ansiPaletteColor(_ index: UInt8) -> Color {
    switch index {
    case 0: return .black
    case 1: return .red
    case 2: return .green
    case 3: return .yellow
    case 4: return .blue
    case 5: return .purple
    case 6: return .teal
    case 7: return Color(white: 0.75)
    case 8: return Color(white: 0.5)
    case 9: return .red
    case 10: return .green
    case 11: return .yellow
    case 12: return .blue
    case 13: return .pink
    case 14: return .cyan
    case 15: return .white
    default:
        let level = Double(index) / 255.0
        return Color(white: level)
    }
}
