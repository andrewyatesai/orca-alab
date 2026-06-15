//! Runnable proof that Orca's native terminal stack is driven by the `aterm`
//! engine. Spawns a real PTY through `orca-session` (whose `HeadlessTerminal`
//! is now an adapter over `aterm-core`), lets the child's output stream through
//! aterm's VT parser into the grid, then renders the grid back out — colours,
//! bold, cursor moves, and erases all performed by aterm.
//!
//! Usage:
//!   orca-aterm-demo                 # run a built-in colour/cursor demo script
//!   orca-aterm-demo <prog> [args…]  # run any program and render its screen

use orca_session::TerminalSession;
use orca_pty::PtyCommand;
use orca_terminal::{Cell, Color};

const ROWS: u16 = 14;
const COLS: u16 = 72;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let command = match argv.split_first() {
        Some((program, rest)) => {
            PtyCommand { program: program.clone(), args: rest.to_vec(), cwd: None, env: Vec::new() }
        }
        None => PtyCommand {
            program: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), demo_script().to_string()],
            cwd: None,
            env: Vec::new(),
        },
    };

    println!(
        "\x1b[2m── spawning `{} {}` in a {ROWS}×{COLS} PTY; \
         output emulated by aterm-core via orca-session ──\x1b[0m",
        command.program,
        command.args.join(" ")
    );

    let mut session = match TerminalSession::spawn(&command, ROWS, COLS) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("spawn failed: {e}");
            std::process::exit(1);
        }
    };
    session.wait(); // child exits, aterm finishes parsing the drained output

    let (rows, cols) = session.size();
    let (cur_row, cur_col) = session.cursor();

    println!("\n┌─ aterm-rendered grid {rows}×{cols}, cursor at ({cur_row},{cur_col}) ─");
    let last = last_nonblank_row(&session, rows);
    for r in 0..=last {
        let text = session.row_text(r);
        let width = text.chars().count();
        print!("│ ");
        for c in 0..width {
            match session.cell(r, c) {
                Some(cell) => print!("{}", render_cell(&cell)),
                None => print!(" "),
            }
        }
        println!();
    }
    println!("└─ every colour, attribute, and cursor move above was produced by aterm ─");

    // Spell out a couple of decoded cells so the proof survives a non-colour
    // terminal / captured log: these attributes came out of aterm's SGR parser.
    println!("\n\x1b[2mdecoded cells (proof the styling is real, not faked):\x1b[0m");
    for (r, c) in [(0usize, 0usize), (1, 5), (2, 5)] {
        if let Some(cell) = session.cell(r, c) {
            println!(
                "  cell[{r}][{c}] = {:?}  bold={} italic={} underline={} fg={:?} bg={:?}",
                cell.ch, cell.attrs.bold, cell.attrs.italic, cell.attrs.underline, cell.attrs.fg,
                cell.attrs.bg
            );
        }
    }
}

/// Built-in script: exercises SGR colours (16/256/truecolor), bold, underline,
/// and cursor-left + erase-to-end-of-line — all VT features the old vte subset
/// did not fully model.
fn demo_script() -> &'static str {
    "printf '\\033[1;32materm\\033[0m is the engine — a real \\033[1;34mPTY\\033[0m, \
emulated headlessly\\n'; \
printf 'sgr: \\033[31mred \\033[32mgreen \\033[34mblue\\033[0m \\033[1mbold\\033[0m \
\\033[3mitalic\\033[0m \\033[4munderline\\033[0m\\n'; \
printf '256: \\033[38;5;208mxx\\033[0m  truecolor: \\033[38;2;255;105;180mxx\\033[0m\\n'; \
printf 'cursor+erase: ABCDEFGH\\033[5D\\033[0KXY\\n'; \
printf 'cwd via OSC-7 set to /tmp\\033]7;file:///tmp\\007\\n'; \
printf 'shell pid = %s\\n' \"$$\""
}

/// Reconstruct an ANSI-styled glyph from an aterm cell so a real terminal shows
/// exactly the colours aterm resolved.
fn render_cell(cell: &Cell) -> String {
    let mut codes: Vec<String> = Vec::new();
    if cell.attrs.bold {
        codes.push("1".into());
    }
    if cell.attrs.italic {
        codes.push("3".into());
    }
    if cell.attrs.underline {
        codes.push("4".into());
    }
    if cell.attrs.inverse {
        codes.push("7".into());
    }
    codes.push(sgr_color(cell.attrs.fg, true));
    codes.push(sgr_color(cell.attrs.bg, false));
    format!("\x1b[{}m{}\x1b[0m", codes.join(";"), cell.ch)
}

fn sgr_color(color: Color, fg: bool) -> String {
    let (base_default, base_index, base_rgb) = if fg { (39, 38, 38) } else { (49, 48, 48) };
    match color {
        Color::Default => base_default.to_string(),
        Color::Indexed(n) => format!("{base_index};5;{n}"),
        Color::Rgb(r, g, b) => format!("{base_rgb};2;{r};{g};{b}"),
    }
}

fn last_nonblank_row(session: &TerminalSession, rows: usize) -> usize {
    (0..rows).rev().find(|&r| !session.row_text(r).is_empty()).unwrap_or(0)
}
