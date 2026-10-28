#ifndef TUI_TEST_WINDOWS_TERMINAL_BRIDGE_H
#define TUI_TEST_WINDOWS_TERMINAL_BRIDGE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#define WT_NOEXCEPT noexcept
#else
#define WT_NOEXCEPT
#endif

#define WT_ABI_VERSION 1
#define WT_COLOR_COUNT 259

typedef struct wt_terminal wt_terminal;

typedef struct wt_error {
    char message[1024];
} wt_error;

typedef struct wt_config {
    uint32_t abi_version;
    uint16_t cols;
    uint16_t rows;
    uint32_t scrollback;
    /* COLORREF order: foreground, background, cursor, then ANSI 0..255. */
    uint32_t colors[WT_COLOR_COUNT];
} wt_config;

typedef struct wt_span {
    size_t offset;
    size_t length;
} wt_span;

typedef struct wt_state {
    uint32_t cols;
    uint32_t rows;
    uint32_t history;
    uint32_t offset;
    uint32_t cursor_x;
    uint32_t cursor_y;
    /* 0 = block, 1 = underline, 2 = bar. */
    uint32_t cursor_shape;
    /* Bits follow the explicit order of tui-test's TerminalMode::ALL. */
    uint32_t modes;
    uint32_t kitty_flags;
    uint32_t colors[WT_COLOR_COUNT];
} wt_state;

typedef struct wt_cell {
    wt_span text;
    wt_span uri;
    wt_span link_id;
    /* 0 = default, 0x01000000 | index, 0x02000000 | COLORREF. */
    uint32_t foreground;
    uint32_t background;
    uint32_t underline_color;
    uint32_t attrs;
    uint32_t underline_style;
} wt_cell;

typedef struct wt_snapshot {
    uint32_t abi_version;
    wt_state state;
    const wt_cell* cells;
    size_t cell_count;
    const uint16_t* strings;
    size_t string_count;
    wt_span title;
    wt_span clipboard;
    uint64_t clipboard_revision;
    void* owner;
} wt_snapshot;

typedef struct wt_output {
    const uint16_t* text;
    size_t length;
    void* owner;
} wt_output;

/* All calls are synchronous, single-owner, and must stay on the creating thread.
   No C++ exception, allocator ownership, or STL type crosses this boundary. */
int wt_create(const wt_config* config, wt_terminal** terminal, wt_error* error) WT_NOEXCEPT;
void wt_destroy(wt_terminal* terminal) WT_NOEXCEPT;
int wt_write(wt_terminal* terminal, const uint16_t* text, size_t length, uint64_t* bells, wt_error* error) WT_NOEXCEPT;
int wt_resize(wt_terminal* terminal, uint16_t cols, uint16_t rows, wt_error* error) WT_NOEXCEPT;
int wt_scroll(wt_terminal* terminal, uint32_t offset, wt_error* error) WT_NOEXCEPT;
/* rows: 0 = metadata only, 1 = viewport, 2 = active screen, 3 = all history. */
int wt_read(wt_terminal* terminal, uint32_t rows, wt_snapshot** snapshot, wt_error* error) WT_NOEXCEPT;
void wt_snapshot_free(wt_snapshot* snapshot) WT_NOEXCEPT;
int wt_take_output(wt_terminal* terminal, wt_output** output, wt_error* error) WT_NOEXCEPT;
void wt_output_free(wt_output* output) WT_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#undef WT_NOEXCEPT
#endif
