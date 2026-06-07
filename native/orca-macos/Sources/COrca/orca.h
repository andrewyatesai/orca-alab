/* Synced from rust/crates/orca-ffi/include/orca.h — keep in lockstep with the
 * orca-ffi crate's C ABI. */
#ifndef ORCA_FFI_H
#define ORCA_FFI_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct OrcaTerminal OrcaTerminal;

typedef struct OrcaColor {
  unsigned char kind; /* 0=default, 1=indexed, 2=truecolor */
  unsigned char index;
  unsigned char r;
  unsigned char g;
  unsigned char b;
} OrcaColor;

typedef struct OrcaCell {
  unsigned int ch;
  unsigned char bold;
  unsigned char italic;
  unsigned char underline;
  unsigned char inverse;
  OrcaColor fg;
  OrcaColor bg;
} OrcaCell;

const char *orca_ffi_version(void);

OrcaTerminal *orca_terminal_new(size_t rows, size_t cols);
void orca_terminal_free(OrcaTerminal *terminal);
void orca_terminal_process(OrcaTerminal *terminal, const unsigned char *bytes, size_t len);
char *orca_terminal_row_text(const OrcaTerminal *terminal, size_t row);
void orca_terminal_cursor(const OrcaTerminal *terminal, size_t *out_row, size_t *out_col);
OrcaCell orca_terminal_cell(const OrcaTerminal *terminal, size_t row, size_t col);
void orca_terminal_size(const OrcaTerminal *terminal, size_t *out_rows, size_t *out_cols);
void orca_terminal_resize(OrcaTerminal *terminal, size_t rows, size_t cols);
void orca_string_free(char *string);

/* ---- Live terminal session: PTY + headless terminal ---- */
typedef struct OrcaSession OrcaSession;

OrcaSession *orca_session_spawn(const char *program, const char *const *args, size_t argc,
                                size_t rows, size_t cols);
void orca_session_free(OrcaSession *session);
void orca_session_wait(OrcaSession *session);
void orca_session_write(const OrcaSession *session, const unsigned char *bytes, size_t len);
void orca_session_resize(const OrcaSession *session, size_t rows, size_t cols);
void orca_session_size(const OrcaSession *session, size_t *out_rows, size_t *out_cols);
void orca_session_cursor(const OrcaSession *session, size_t *out_row, size_t *out_col);
char *orca_session_row_text(const OrcaSession *session, size_t row);
OrcaCell orca_session_cell(const OrcaSession *session, size_t row, size_t col);

#ifdef __cplusplus
}
#endif

#endif /* ORCA_FFI_H */
