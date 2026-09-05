#ifndef TUI_TEST_GO_NATIVE_H
#define TUI_TEST_GO_NATIVE_H

#pragma once

#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * A borrowed UTF-8 byte string. NULL denotes absence; a non-NULL pointer
 * with zero length denotes an explicitly empty string. Inputs live through
 * the call only. All output pointers live until tui_result_free.
 */
typedef struct TuiString {
  const uint8_t *data;
  size_t len;
} TuiString;

typedef struct TuiOptionalI32 {
  bool present;
  int32_t value;
} TuiOptionalI32;

typedef struct TuiOptionalU64 {
  bool present;
  uint64_t value;
} TuiOptionalU64;

typedef struct TuiOpenResult {
  struct TuiOptionalU64 shell_pid;
  struct TuiString session;
  bool ready;
  struct TuiString recording;
} TuiOpenResult;

typedef struct TuiCursor {
  uint16_t x;
  uint16_t y;
} TuiCursor;

typedef struct TuiTimeouts {
  struct TuiOptionalU64 text;
  struct TuiOptionalU64 idle;
  struct TuiOptionalU64 command;
  struct TuiOptionalU64 exit;
  struct TuiOptionalU64 ready;
} TuiTimeouts;

typedef struct TuiState {
  struct TuiString session_shell;
  uint16_t cols;
  uint16_t rows;
  struct TuiCursor cursor;
  struct TuiString title;
  struct TuiString cwd;
  struct TuiString last_command;
  struct TuiOptionalI32 last_exit;
  struct TuiOptionalI32 exited;
  bool ready;
  uint64_t bell_count;
  struct TuiTimeouts timeouts;
  struct TuiString text;
} TuiState;

typedef struct TuiSize {
  uint16_t cols;
  uint16_t rows;
} TuiSize;

/**
 * kind: 0 default, 1 indexed (index), 2 RGB (red/green/blue).
 */
typedef struct TuiColor {
  uint32_t kind;
  uint8_t index;
  uint8_t red;
  uint8_t green;
  uint8_t blue;
} TuiColor;

typedef struct TuiCell {
  uint16_t x;
  uint16_t y;
  struct TuiString character;
  struct TuiColor fg;
  struct TuiColor bg;
  bool bold;
  bool dim;
  bool italic;
  bool inverse;
  bool invisible;
  bool strike;
  bool blink;
  bool underline;
  struct TuiString underline_style;
  struct TuiColor underline_color;
} TuiCell;

typedef struct TuiPosition {
  uint32_t row;
  uint16_t column;
} TuiPosition;

typedef struct TuiSpan {
  uint32_t row;
  uint16_t start;
  uint16_t end;
} TuiSpan;

typedef struct TuiMatch {
  struct TuiString text;
  struct TuiPosition start;
  struct TuiPosition end;
  const struct TuiSpan *spans;
  size_t spans_len;
} TuiMatch;

typedef struct TuiBellEvent {
  uint64_t sequence;
  uint64_t elapsed_ms;
} TuiBellEvent;

/**
 * The function called determines the populated success field. error_kind is
 * 0 on success, 1 assertion, 2 usage, 3 no-session, 5 internal.
 * snapshot: 0 passed, 1 written, 2 updated. Free exactly once.
 */
typedef struct TuiResult {
  uint32_t error_kind;
  struct TuiString error_message;
  struct TuiString text;
  uint64_t number;
  struct TuiOptionalI32 exit_code;
  struct TuiOpenResult open;
  struct TuiState state;
  struct TuiCursor cursor;
  struct TuiSize size;
  const struct TuiCell *cells;
  size_t cells_len;
  const struct TuiMatch *matches;
  size_t matches_len;
  const struct TuiBellEvent *bells;
  size_t bells_len;
  const struct TuiString *strings;
  size_t strings_len;
  uint32_t snapshot;
  void *private_data;
} TuiResult;

typedef struct TuiPair {
  struct TuiString key;
  struct TuiString value;
} TuiPair;

typedef struct TuiOptionalBool {
  bool present;
  bool value;
} TuiOptionalBool;

typedef struct TuiOpenOptions {
  struct TuiString backend;
  struct TuiString shell;
  struct TuiOptionalU64 cols;
  struct TuiOptionalU64 rows;
  struct TuiString cwd;
  const struct TuiPair *env;
  size_t env_len;
  struct TuiOptionalBool wait_ready;
  bool restart;
  struct TuiOptionalU64 scrollback;
  const struct TuiPair *colors;
  size_t colors_len;
  struct TuiTimeouts timeouts;
  struct TuiString recording_mode;
  struct TuiString recording_directory;
} TuiOpenOptions;

/**
 * button: 0 left, 1 middle, 2 right.
 */
typedef struct TuiMouseOptions {
  uint32_t button;
  bool alt;
  bool ctrl;
  bool shift;
} TuiMouseOptions;

typedef struct TuiWaitOptions {
  struct TuiOptionalU64 timeout_ms;
  bool regex;
  bool not;
} TuiWaitOptions;

/**
 * occurrence: 0 any, 1 unique, 2 first, 3 last, 4 nth.
 */
typedef struct TuiAnchor {
  struct TuiString text;
  bool regex;
  uint32_t occurrence;
  size_t index;
} TuiAnchor;

typedef struct TuiTextStyle {
  struct TuiString foreground;
  struct TuiString background;
  struct TuiOptionalBool bold;
  struct TuiOptionalBool dim;
  struct TuiOptionalBool italic;
  struct TuiString underline_style;
  struct TuiString underline_color;
  struct TuiOptionalBool inverse;
  struct TuiOptionalBool hidden;
  struct TuiOptionalBool strikethrough;
  struct TuiOptionalBool blink;
} TuiTextStyle;

/**
 * kind: 0 text, 1 style. direction: 0 within, 1 after, 2 before.
 * whitespace: 0 exact, 1 normalize. Stages are ordered parent first.
 */
typedef struct TuiLocatorStage {
  uint32_t kind;
  struct TuiString text;
  bool regex;
  bool full;
  uint32_t whitespace;
  struct TuiAnchor after;
  struct TuiAnchor before;
  struct TuiTextStyle style;
  uint32_t occurrence;
  size_t index;
  uint32_t direction;
} TuiLocatorStage;

typedef struct TuiQuery {
  const struct TuiLocatorStage *stages;
  size_t len;
} TuiQuery;

typedef struct TuiOptionalF64 {
  bool present;
  double value;
} TuiOptionalF64;

typedef struct TuiRecordingOptions {
  struct TuiString path;
  struct TuiString format;
  struct TuiOptionalU64 fps;
  struct TuiOptionalF64 speed;
  struct TuiOptionalF64 idle_time_limit;
  struct TuiOptionalF64 zoom;
} TuiRecordingOptions;

uint32_t tui_abi_version(void);

/**
 * # Safety
 * result must be NULL or a live result returned by this library, freed once.
 */
void tui_result_free(struct TuiResult *result);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_open(struct TuiString session, struct TuiOpenOptions options);

/**
 * Pointer form for foreign callers with limited by-value argument space.
 *
 * # Safety
 * options must be NULL or point to a valid TuiOpenOptions for this call.
 * Its borrowed buffers follow the same contract as tui_open.
 */
struct TuiResult *tui_open_ptr(struct TuiString session, const struct TuiOpenOptions *options);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_run(struct TuiString session,
                          struct TuiOpenOptions options,
                          struct TuiString program,
                          const struct TuiString *args,
                          size_t args_len);

/**
 * Pointer form for foreign callers with limited by-value argument space.
 *
 * # Safety
 * options must be NULL or point to a valid TuiOpenOptions for this call.
 * All borrowed buffers follow the same contract as tui_run.
 */
struct TuiResult *tui_run_ptr(struct TuiString session,
                              const struct TuiOpenOptions *options,
                              struct TuiString program,
                              const struct TuiString *args,
                              size_t args_len);

struct TuiResult *tui_sessions(void);

struct TuiResult *tui_close_all(void);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_recording(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_close(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_state(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_get_command(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_get_output(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_get_exit_code(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_get_cwd(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_get_cursor(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_get_size(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_get_title(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_get_clipboard(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_get_bell_count(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_get_bell_events(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_stop_recording(struct TuiString session);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_text(struct TuiString session, bool full);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_packed_screen(struct TuiString session, bool full);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_cells(struct TuiString session,
                            uint16_t x,
                            uint16_t y,
                            uint16_t w,
                            uint16_t h);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_write(struct TuiString session, struct TuiString text);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_submit(struct TuiString session, struct TuiString text);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_signal(struct TuiString session, struct TuiString text);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_key(struct TuiString session,
                          const struct TuiString *keys,
                          size_t len,
                          uint32_t action);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_resize(struct TuiString session, uint16_t cols, uint16_t rows);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_mouse_click(struct TuiString session,
                                  struct TuiOptionalU64 x,
                                  struct TuiOptionalU64 y,
                                  struct TuiString on_text,
                                  struct TuiMouseOptions options,
                                  uint8_t clicks);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_mouse_move(struct TuiString session, uint16_t x, uint16_t y);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_mouse_down(struct TuiString session,
                                 uint16_t x,
                                 uint16_t y,
                                 struct TuiMouseOptions options);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_mouse_up(struct TuiString session,
                               uint16_t x,
                               uint16_t y,
                               struct TuiMouseOptions options);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_mouse_drag(struct TuiString session,
                                 uint16_t x1,
                                 uint16_t y1,
                                 uint16_t x2,
                                 uint16_t y2,
                                 struct TuiMouseOptions options);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_mouse_scroll(struct TuiString session,
                                   struct TuiString direction,
                                   uint16_t amount);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_wait_title(struct TuiString session,
                                 struct TuiString text,
                                 struct TuiWaitOptions options);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_expect_title(struct TuiString session,
                                   struct TuiString text,
                                   struct TuiWaitOptions options);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_wait_clipboard(struct TuiString session,
                                     struct TuiString text,
                                     struct TuiWaitOptions options);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_wait_idle(struct TuiString session, struct TuiOptionalU64 timeout);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_wait_command(struct TuiString session, struct TuiOptionalU64 timeout);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_wait_exit(struct TuiString session, struct TuiOptionalU64 timeout);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_wait_ready(struct TuiString session, struct TuiOptionalU64 timeout);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_wait_bell(struct TuiString session, struct TuiOptionalU64 timeout);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_find_locator(struct TuiString session, struct TuiQuery query);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_wait_locator(struct TuiString session,
                                   struct TuiQuery query,
                                   struct TuiWaitOptions options);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_click_locator(struct TuiString session,
                                    struct TuiQuery query,
                                    struct TuiMouseOptions options,
                                    uint8_t clicks,
                                    struct TuiOptionalU64 timeout);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_highlight_locator(struct TuiString session,
                                        struct TuiQuery query,
                                        struct TuiOptionalU64 timeout);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_expect_exit_code(struct TuiString session,
                                       int32_t code,
                                       struct TuiOptionalU64 timeout);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_expect_output(struct TuiString session, struct TuiString text, bool regex);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_expect_bell_count(struct TuiString session,
                                        uint64_t count,
                                        struct TuiOptionalU64 timeout);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_snapshot(struct TuiString session,
                               struct TuiString name,
                               bool update,
                               bool include_colors,
                               bool include_title,
                               struct TuiString cwd);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_screenshot(struct TuiString session,
                                 bool full,
                                 struct TuiString path,
                                 struct TuiOptionalF64 zoom);

/**
 * # Safety
 * Borrowed input buffers must be valid and readable for their stated lengths
 * throughout this call; see TuiString and the input structure contracts.
 */
struct TuiResult *tui_start_recording(struct TuiString session, struct TuiRecordingOptions options);

#endif  /* TUI_TEST_GO_NATIVE_H */
