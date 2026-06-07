import OrcaKit
import OrcaUI
import SwiftUI

/// The thin macOS shell: a SwiftUI app that runs the user's shell in a live PTY
/// session (in the Rust core) and renders it. All terminal logic lives in Rust;
/// this app owns only the window, a redraw tick, and key-input forwarding.
@main
struct OrcaApp: App {
    var body: some Scene {
        WindowGroup("Orca") {
            ContentView()
                .frame(minWidth: 720, minHeight: 432)
        }
    }
}

/// Owns the live session and a redraw tick. Bumping `revision` re-reads the
/// Rust grid as PTY output streams in.
@MainActor
final class TerminalModel: ObservableObject {
    let session: OrcaSession
    @Published var revision = 0
    private var timer: Timer?

    init(rows: Int = 40, cols: Int = 120) {
        let shell = ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/sh"
        session =
            OrcaSession(program: shell, args: ["-l"], rows: rows, cols: cols)
            ?? OrcaSession(program: "/bin/sh", args: [], rows: rows, cols: cols)!
        // ~30fps redraw; the headless terminal is updated by the reader thread,
        // so we just re-read it on a tick.
        timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 30.0, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.revision &+= 1 }
        }
    }

    func send(_ text: String) {
        session.write(text)
    }
}

struct ContentView: View {
    @StateObject private var model = TerminalModel()

    var body: some View {
        SessionTerminalView(session: model.session, revision: model.revision)
            .padding(6)
            .background(Color.black)
            .onKeyPress { press in
                let text = press.characters.isEmpty ? keyText(press.key) : press.characters
                if !text.isEmpty {
                    model.send(text)
                    return .handled
                }
                return .ignored
            }
    }

    /// Map non-character keys to the bytes a shell expects.
    private func keyText(_ key: KeyEquivalent) -> String {
        switch key {
        case .return: return "\r"
        case .tab: return "\t"
        case .delete: return "\u{7f}"
        case .escape: return "\u{1b}"
        case .upArrow: return "\u{1b}[A"
        case .downArrow: return "\u{1b}[B"
        case .rightArrow: return "\u{1b}[C"
        case .leftArrow: return "\u{1b}[D"
        default: return ""
        }
    }
}
