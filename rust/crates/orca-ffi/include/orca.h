/* orca-ffi — C ABI for the Orca Rust core.
 *
 * Consumed by the thin native platform wrappers (SwiftUI on macOS, etc.).
 * Link against the staticlib/cdylib produced by the `orca-ffi` crate.
 *
 * This first surface exposes the headless terminal (orca-terminal): create a
 * terminal, feed it PTY output bytes, read back grid rows / cursor for
 * rendering. Strings returned by orca_* functions must be released with
 * orca_string_free(); the version string is static and must NOT be freed.
 */
#ifndef ORCA_FFI_H
#define ORCA_FFI_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque headless terminal handle. */
typedef struct OrcaTerminal OrcaTerminal;

/* A terminal color: kind 0=default, 1=indexed (`index`), 2=truecolor (r,g,b). */
typedef struct OrcaColor {
  unsigned char kind;
  unsigned char index;
  unsigned char r;
  unsigned char g;
  unsigned char b;
} OrcaColor;

/* A grid cell for native rendering: scalar char + SGR attributes. */
typedef struct OrcaCell {
  unsigned int ch;
  unsigned char bold;
  unsigned char italic;
  unsigned char underline;
  unsigned char inverse;
  OrcaColor fg;
  OrcaColor bg;
} OrcaCell;

/* Library version (static; do NOT free). */
const char *orca_ffi_version(void);

/* Lifecycle. */
OrcaTerminal *orca_terminal_new(size_t rows, size_t cols);
void orca_terminal_free(OrcaTerminal *terminal);

/* Feed PTY output bytes into the grid. */
void orca_terminal_process(OrcaTerminal *terminal, const unsigned char *bytes, size_t len);

/* Row text (trailing blanks trimmed); free with orca_string_free(). */
char *orca_terminal_row_text(const OrcaTerminal *terminal, size_t row);

/* Cursor position; out_row/out_col may be NULL. */
void orca_terminal_cursor(const OrcaTerminal *terminal, size_t *out_row, size_t *out_col);

/* Cell at (row, col) with char + SGR attributes (blank default if OOB/NULL). */
OrcaCell orca_terminal_cell(const OrcaTerminal *terminal, size_t row, size_t col);

/* Grid dimensions; out_rows/out_cols may be NULL. */
void orca_terminal_size(const OrcaTerminal *terminal, size_t *out_rows, size_t *out_cols);

/* Resize the grid. */
void orca_terminal_resize(OrcaTerminal *terminal, size_t rows, size_t cols);

/* Release a string returned by an orca_* function. */
void orca_string_free(char *string);

/* ---- Live terminal session: PTY + headless terminal ---- */
typedef struct OrcaSession OrcaSession;

/* Spawn `program` with `args` (argc entries) in a PTY; output streams into the
 * session's terminal. NULL on spawn failure; free with orca_session_free. */
OrcaSession *orca_session_spawn(const char *program, const char *const *args, size_t argc,
                                size_t rows, size_t cols);
void orca_session_free(OrcaSession *session);
/* Wait for the child to exit and all output to drain. */
void orca_session_wait(OrcaSession *session);
void orca_session_write(const OrcaSession *session, const unsigned char *bytes, size_t len);
void orca_session_resize(const OrcaSession *session, size_t rows, size_t cols);
void orca_session_size(const OrcaSession *session, size_t *out_rows, size_t *out_cols);
void orca_session_cursor(const OrcaSession *session, size_t *out_row, size_t *out_col);
char *orca_session_row_text(const OrcaSession *session, size_t row);
OrcaCell orca_session_cell(const OrcaSession *session, size_t row, size_t col);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* ORCA_FFI_H */
