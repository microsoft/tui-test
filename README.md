# tui-test

`tui-test` controls, inspects, tests, and records real shell sessions and full-screen terminal apps on Windows, Linux, and macOS. Use it from the CLI or call the same engine from Rust, Python, or JavaScript. It works for AI agents that need structured access to terminal state, terminal automation, and terminal ui application testing.

<p align="center">
  <a href="#installation">Installation</a>
  ·
  <a href="#quick-start">Quick start</a>
  ·
  <a href="#built-for-ai-agents">AI agents</a>
  ·
  <a href="#api-references">API references</a>
  ·
  <a href="#configuration">Configuration</a>
</p>

> [!IMPORTANT]
> `tui-test` is undergoing a major rewrite. These docs cover the beta releases.

## Features

- Give AI agents structured terminal state, machine-readable output, perform common terminal actions, stable exit codes, and a CLI-generated skill.
- Send keyboard and mouse input to shells and full-screen TUI programs, resize the terminal, or signal the child process.
- Read terminal text, cell colors, cursor position, window titles, command output, and exit codes.
- Wait for terminal state and assert against text, colors, snapshots, command output, or exit status.
- Save SVG screenshots, APNG/GIF/MP4 recordings, or asciinema casts.

## Installation

Use the CLI from shell scripts and agent workflows. The Rust, Python, and JavaScript packages use the in-process API and do not require the CLI.

<details>
<summary><strong>CLI</strong></summary>

### macOS and Linux

#### homebrew 
```sh
brew tap microsoft/tui-test https://github.com/microsoft/tui-test
brew install tui-test
```

#### bash
```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/microsoft/tui-test/main/install/install.sh | TUI_TEST_VERSION=beta sh
```
Set `TUI_TEST_VERSION` to install a specific version or `TUI_TEST_INSTALL_DIR` to choose the install location.

### Windows

#### powershell
```powershell
$env:TUI_TEST_VERSION = "beta"
irm https://raw.githubusercontent.com/microsoft/tui-test/main/install/install.ps1 | iex
```
Set `TUI_TEST_VERSION` to install a specific version or `TUI_TEST_INSTALL_DIR` to choose the install location.

### Release binaries

Download the latest beta for your platform from [GitHub Releases](https://github.com/microsoft/tui-test/releases).

</details>

<details>
<summary><strong>Rust</strong></summary>

Install [`tui-test-rs`](https://crates.io/crates/tui-test-rs):

```sh
cargo add tui-test-rs@0.1.0-beta.2
# Add APNG/GIF/MP4 export support when the Rust application needs raster recording:
cargo add tui-test-rs@0.1.0-beta.2 --features recording-raster
```

Raster recording uses installed system fonts unless a JetBrains Mono bundle feature is enabled:

| Feature | Bundled JetBrains Mono faces |
| --- | --- |
| `recording-raster` | None; use installed system fonts |
| `recording-font-jetbrains-mono` | Full-glyph Regular |
| `recording-font-jetbrains-mono-styles` | Regular, Bold, Italic, and Bold Italic |
| `recording-font-jetbrains-mono-full` | All 16 static family faces |

Each font feature enables `recording-raster`; the tiers are cumulative.

</details>

<details>
<summary><strong>Python</strong></summary>

Install [`tui-test`](https://github.com/microsoft/tui-test/blob/main/bindings/python/README.md) for Python 3.8 or later:

```sh
pip install --pre tui-test
```

</details>

<details>
<summary><strong>JavaScript</strong></summary>

Install [`@microsoft/tui-test`](https://github.com/microsoft/tui-test/blob/main/bindings/js/README.md):

```sh
npm install @microsoft/tui-test@beta # Node 20+

bun add @microsoft/tui-test@beta # Bun (best effort)

deno add npm:@microsoft/tui-test@beta # Deno 2 (best effort)
```

Node is the supported runtime. Bun and Deno support is best effort. Deno needs a local `node_modules` directory and `--allow-ffi` to load the native addon.

</details>

## Quick start

<details>
<summary><strong>CLI</strong></summary>

Run a command and check the result:

```sh
tui-test open                  # start a shell session (auto-starts the daemon)
tui-test submit "echo hello"   # type the command, press Enter
tui-test wait command          # block until it finishes
tui-test expect text "hello"   # assert it showed up
tui-test expect exit-code 0    # assert it exited 0
tui-test close
```

Drive a full-screen TUI:

```sh
tui-test run vim file.txt
tui-test wait idle             # let the screen settle
tui-test key press i
tui-test type "some text"
tui-test key press Escape : w q Enter
tui-test wait exit
```

</details>

<details>
<summary><strong>Rust</strong></summary>

```rust
use tui_test::{OpenOptions, Operation, Session};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = Session::new(format!("rust-example-{}", std::process::id()));
    session.open(OpenOptions::default())?;
    session.execute(Operation::Submit {
        data: Some("echo hello".into()),
    })?;
    let hello = session.get_by_text("hello");
    hello.wait_with_timeout(Some(5_000))?;
    hello.last().expect()?;
    hello.last().highlight()?;
    session.execute(Operation::ExpectExitCode {
        code: 0,
        timeout_ms: Some(5_000),
    })?;
    session.close()?;
    Ok(())
}
```

</details>

<details>
<summary><strong>Python</strong></summary>

```python
import asyncio
from tui_test import TuiTest

async def main():
    async with TuiTest() as su:
        await su.open()
        await su.submit("echo hello")
        hello = su.get_by_text("hello")
        await hello.wait()
        await hello.last().highlight()
        await su.expect_exit_code(0)

asyncio.run(main())
```

</details>

<details>
<summary><strong>JavaScript</strong></summary>

```js
import { TuiTest } from "@microsoft/tui-test";

const su = new TuiTest();
await su.open();
await su.submit("echo hello");
const hello = su.getByText("hello");
await hello.wait();
await hello.last().highlight();
await su.expectExitCode(0);
await su.close();
```

</details>

## Built for AI agents

`tui-test` exposes terminal state through commands instead of making an agent scrape a stream of ANSI output. It tracks command boundaries, exit codes, the working directory, prompts, window titles, cursor position, cells, colors, and bell events.

| Task | Commands |
| --- | --- |
| Start or reuse a terminal | `open`, `run`, `sessions` |
| Inspect what happened | `state`, `text`, `find text`, `cells`, `get`, `screenshot` |
| Interact with the program | `click text`, `submit`, `type`, `key`, `mouse`, `resize`, `signal` |
| Wait for real terminal state | `expect text`, `wait command`, `wait ready`, `wait idle`, `wait title`, `wait bell` |
| Check the result | `expect text`, `expect output`, `expect exit-code`, `expect snapshot` |
| Hand the session to a person | `monitor`, `screenshot`, `record` |

Wait commands react to the terminal instead of relying on fixed sleeps. `--json` returns machine-readable results, and stable [exit codes](#exit-codes) distinguish a failed assertion from a missing session or daemon error.

| Agent command | Description |
| --- | --- |
| `agent-context` | Print versioned JSON for every command, flag, enum, default, and exit code. The installed CLI generates the JSON, so it matches that version. |
| `usage` | Print a one-screen cheatsheet. |
| `skill` | Print the full workflow guide from [`SKILL.md`](https://github.com/microsoft/tui-test/blob/main/SKILL.md). |

### Install the agent skill

```sh
tui-test skill --add
```

## API references

For programmatic use, the Rust API docs and binding READMEs cover the same terminal operations as the CLI reference.

| Surface | Reference |
| --- | --- |
| CLI | [CLI reference](#cli-reference) |
| Rust | [`tui-test-rs` API documentation](https://docs.rs/tui-test-rs/latest/tui_test/) |
| Python | [Python binding README](bindings/python/README.md#api) |
| JavaScript | [JavaScript binding README](bindings/js/README.md#api) |

## CLI reference

`--session <name>` selects a session (env `TUI_TEST_SESSION`, default `default`). `--json` makes output machine-readable. Use `--verbose` or `-v` to log PTY traffic (see [Debugging](#debugging)).

### Sessions

#### Timeouts

Waits and assertions use five timeout classes:

| Class | Applies to | Default |
| --- | --- | --- |
| `text` | text find/expect/click/highlight, locator waits, title, and bell operations | 5000 ms |
| `idle` | `wait idle` | 5000 ms |
| `command` | `wait command`, `expect exit-code` | 30000 ms |
| `exit` | `wait exit` | 30000 ms |
| `ready` | `wait ready`, and the prompt wait inside `open` | 30000 ms |

`open`'s prompt wait caps at 8000 ms unless you set a `ready` timeout.

Configure timeouts directly via cli falgs or within [profiles](#profiles) in the configuration. The timeout priority goes from explicit overrides -> profile -> timeout overrides -> `TUI_TEST_TIMEOUT_<CLASS>_MS` (only affects daemon on start). A session's timeouts can be viewed with the `tui-test state` command.

```sh
tui-test open --timeout-text 30000 --timeout-idle 15000 --timeout-ready 20000
tui-test expect text "done" --timeout 60000 # just this call
```

#### Lifecycle

| Command                                                      | Description                                 |
| ------------------------------------------------------------ | ------------------------------------------- |
| `open [--shell S] [--backend B] [--cols N --rows N] [--cwd D] [--env K=V] [--config F] [--profile P] [--timeout-<class> MS] [--restart]` | Spawn or reuse a shell session.             |
| `run [--backend B] [--config F] [--profile P] [--restart] <program> [args...]` | Spawn or reuse a session running a program. |
| `sessions`                                                   | List active sessions.                       |
| `close [--all]`                                              | Close the current session (or all).         |
| `daemon start` / `daemon status` / `daemon stop --session N \| --all` | Start, inspect, or stop a session's daemon. |

Each session has its own daemon. `close` stops that daemon. To stop one directly, pass `--session <name>` or `--all` to `daemon stop`.

If a session's daemon comes from another `tui-test` version, the client replaces it before sending the command. A per-session lock prevents concurrent clients from racing during the restart.

`open` waits for a prompt before returning, `run` does not. Override with `--wait-ready` / `--no-wait-ready`. An explicit `--wait-ready` fails (exit 1) if no prompt appears; `open`'s implicit wait reports `ready` in its payload either way.

Calling `open` or `run` for a session that already has a live child reuses that child. Pass `--restart` (or its `--force` alias) to replace it explicitly.

#### Terminal backends

Select a backend per session with `--backend`. The available values are `alacritty` (default), `ghostty`, `rio`, and `xtermjs`; Ghostty uses [Ghostty's Rust VT bindings](https://github.com/Uzaaft/libghostty-rs), and `xtermjs` runs [xterm.js](https://xtermjs.org) inside [QuickJS](https://bellard.org/quickjs/).

```sh
tui-test open --backend ghostty
tui-test run --backend xtermjs vim file.txt
```

All backends use the same renderer, assertions, snapshot format, and conformance suite. Shell semantic-prompt tracking reads raw PTY bytes, so command boundaries, exit codes, and cwd tracking do not depend on the selected backend. Backend-specific VT behavior can differ: Ghostty preserves SGR blink, while Alacritty parses blink but cannot report it. xterm.js records a cell's underline color only when that cell also has an underline style.

The CLI and published Python and JavaScript packages include all backends. Windows ARM64 artifacts are not currently published because Ghostty's upstream Zig build does not support that target.

Rust users enable non-default backends through Cargo features. To enable Ghostty:

```sh
cargo add tui-test-rs --features rio
cargo add tui-test-rs --features xtermjs
```

```rust
use tui_test::{Backend, OpenOptions};

let options = OpenOptions {
    backend: Backend::Rio,
    ..OpenOptions::default()
};
```

The default Rust features include only Alacritty and do not require Zig. The `ghostty` feature builds a pinned Ghostty revision and requires Zig 0.16 on `PATH`.

### Terminal control

#### Inspection

| Command                                             | Description                                                                                 |
| --------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `state`                                             | cwd, size, cursor, window title, last command + exit code, bell count, effective timeouts, text snapshot. |
| `text [--full]`                                     | Plain text of the viewport (or scrollback).                                                 |
| `find text "T" [selector/style options]`            | Return current matches with zero-based row/column spans.                                    |
| `screenshot [-o file.svg] [--full] [--zoom N]`      | Terminal text to stdout, or a full-color SVG scaled without changing its terminal cells.   |
| `cells X Y [W H]`                                   | Per-cell attributes (char, fg, bg, flags).                                                  |
| `get command\|output\|exit-code\|cwd\|cursor\|size\|title\|bells\|bell-events` | Structured getters.                                                                   |

`state` prints `key: value` lines then the screen; `text` and `screenshot` print the screen bare.

#### Input

| Command                                                       | Description                                                  |
| ------------------------------------------------------------- | ------------------------------------------------------------ |
| `type "text"`                                                 | Type literal text.                                         |
| `submit ["text"]`                                             | Type then press the shell return key.                      |
| `key press <Key...>`                                          | Simulate key presses, e.g. `key press Ctrl+C`.             |
| `key down <Key...>` / `key up <Key...>`                       | Simulate explicit keydown and keyup events.                |
| `key repeat <Key...>`                                         | Send repeat events (press-equivalent in legacy mode).      |
| `click text "T" [selector/style options]`                      | Auto-wait for one match and click its middle cell.         |
| `mouse click X Y` / `mouse click --on-text "OK" [--clicks N]` | Click by coords or label.                                  |
| `mouse move\|down\|up\|drag\|scroll ...`                      | Full mouse control.                                        |

Key input follows the Kitty keyboard protocol negotiated by the child. A `key press` sends the normal press input and adds a release only when the child requests Kitty event-type reporting. Text-producing keys also require report-all-keys mode before repeat and release events can be represented. Modifiers are `Ctrl`, `Alt` / `Option`, `Shift`, `Super`, `Hyper`, and `Meta`; the top-level `press` command remains a compatibility alias for `key press`.

#### Text queries and actions

Text commands share the same selector and optional style flags:

```sh
tui-test find text "Save"                                    # current locations
tui-test expect text "Save" --fg green                       # assert green Save
tui-test click text "Save" --fg green --timeout 5000         # auto-wait, then click
tui-test highlight text 'item\s+\d+' --regex                 # highlight every match
```

Selectors support exact or normalized whitespace, full scrollback, regular
expressions, `after` / `before` anchors, and
`any|unique|first|last|nth` occurrence selection. Click is strict unless the
selector chooses one occurrence. Highlighted cells appear in the live monitor
and SVG screenshots until the terminal redraws. `find`, `expect`, and
`highlight` default to any match; use `--match unique` when exactly one is
required.

The Rust, Python, and JavaScript APIs expose chainable `get_by_text` /
`getByText` and `get_by_style` / `getByStyle` locators. Each stage searches
`within`, `after`, or `before` the dynamically resolved parent match. Rust
uses `get_by_text_relative(..., LocatorDirection::After)`; Python and
JavaScript pass `direction="after"` / `{ direction: "after" }`.
Programmatic selectors always start with all matches, and occurrence selection
is expressed only by chaining `any`, `unique`, `first`, `last`, or `nth`.
`expect` succeeds when any selected match exists; add `unique` to require
exactly one. Style expectations are composed with a nested `get_by_style` /
`getByStyle` locator. With the default `within` direction, that stage filters
and preserves parent matches whose visible cells all satisfy the style.
Negation is an expectation option, not a locator selector; combining it with
`unique` means zero matches pass, one present match fails, and multiple matches
remain ambiguous. Use `count()` with the host test framework to assert an
exact current count.

#### PTY control

| Command                           | Description                      |
| --------------------------------- | -------------------------------- |
| `resize COLS ROWS`                | Resize the PTY and emulator.     |
| `write <data>`                    | Write raw bytes (no return key). |
| `signal INT\|TERM\|KILL` / `kill` | Signal / kill the child.         |

### Waiting and assertions

#### Wait

| Command                                             | Description                         |
| --------------------------------------------------- | ----------------------------------- |
| `wait title "T" [--regex --not --timeout MS]`       | Until the window title (OSC 0/2) matches. |
| `wait idle`                                         | Until the screen stops changing.    |
| `wait command`                                      | Until the current command finishes. |
| `wait exit`                                         | Until the session exits.            |
| `wait ready`                                        | Until the shell reports a prompt.   |
| `wait bell`                                         | Until the next terminal bell event. |

#### Expect (exit 0 = pass, 1 = fail)

| Command                                                                         | Description                                |
| ------------------------------------------------------------------------------- | ------------------------------------------ |
| `expect text "T" [selector/style options] [--not --timeout MS]` | Retry a text-and-style assertion.           |
| `expect title "T" [--regex --not --timeout MS]`                                 | Window title set with OSC 0/2.             |
| `expect exit-code N [--timeout MS]`                                             | Last command's exit code.                  |
| `expect output "T" [--regex]`                                                   | Last command's captured output.            |
| `expect bell N [--timeout MS]`                                                  | Cumulative bell count reaches at least N.  |
| `expect snapshot NAME [-u] [--include-colors --include-title]`                                  | Compare against `__snapshots__/NAME.snap`. `--include-title` adds the window title to the frame. |

Colors accept ANSI-256 (`9`), hex (`#ff0000`), or RGB (`255,0,0`).

### Captures

#### Screenshots

Without `-o`, `screenshot` draws the session in the current terminal. Pass `-o file.svg` to save a full-color SVG instead. `--zoom 0.5` halves the image dimensions without changing the terminal's rows or columns. Nerd Font icons are embedded as vector paths, so the SVG does not need an installed Nerd Font.

`tui-test` appends `COLSxROWS` to the program title in rendered screenshots and recordings. When the terminal has no title, it uses `tui-test capture - COLSxROWS`.

<p align="center">
  <img alt="full-color SVG screenshot of a TUI rendered by tui-test" src="static/screen.svg" width="400">
</p>

#### Recording

`record` writes a selected part of a session to animated APNG (primary), GIF (fallback), MP4 video, or an [asciinema v2](https://docs.asciinema.org/manual/asciicast/v2/) cast:

| Command | Description |
| --- | --- |
| `record start OUT [--format apng\|gif\|mp4\|cast] [--fps N] [--speed N] [--idle-time-limit SEC] [--zoom N]` | Start recording. Format is inferred from `.png`/`.apng`, `.gif`, `.mp4`, or `.cast`. |
| `record stop` | Stop recording and finish the output file. |
| `get-recording [session]` | Print the separate, always-on session cast to stdout. |

```sh
tui-test open
tui-test record start demo.png --zoom 0.5
tui-test submit "echo hello"
tui-test wait command
tui-test record stop
```

`--zoom` scales SVG screenshots and APNG/GIF/MP4 output without changing the terminal's rows or columns, and MP4 export requires `ffmpeg` on `PATH`. The `recording-font-jetbrains-mono*` features bundle JetBrains Mono for raster exports; set `TUI_TEST_RECORDING_FONT_FAMILIES=Family One,Family Two` to prefer installed font families.

<p align="center">
  <img alt="animated APNG terminal recording produced by tui-test" src="static/recording.png" width="400">
</p>

The same 48x10-cell recording rendered at native 100%, 50%, and 25% zoom:

| 100% | 50% | 25% |
| --- | --- | --- |
| <img alt="terminal recording rendered at 100 percent zoom" src="static/recording-zoom-100.png"> | <img alt="terminal recording rendered at 50 percent zoom" src="static/recording-zoom-50.png"> | <img alt="terminal recording rendered at 25 percent zoom" src="static/recording-zoom-25.png"> |

The output canvas keeps the same dimensions when the terminal is resized. Existing content reflows inside it:

<p align="center">
  <img alt="animated GIF showing a centered terminal window resizing" src="static/resize-demo.gif" width="400">
</p>

### Live monitor

Watch a live session in a second terminal while an agent or user drives it. Both share the same daemon. `monitor` takes over an alternate screen and streams the session. Programmtic sessions can view state view recordings, not live monitoring

https://github.com/user-attachments/assets/741c985f-7861-41c5-9ceb-0f82f705b43f

| Command   | Description                                                                       |
| --------- | --------------------------------------------------------------------------------- |
| `monitor` | Attach a live, full-color framed view of the session (`--session` selects which). |

```sh
tui-test --session work monitor   # watch the 'work' session live
```

`monitor` needs an interactive terminal (exit `2` otherwise) and an existing session (exit `3` if none). It only reads shared screen state, so it does not block commands sent by the other client. Resizing the monitor window refits the frame without resizing the session.

### Exit codes

`tui-test` uses the same exit codes across commands:

| Code | Meaning                                               |
| ---- | ----------------------------------------------------- |
| `0`  | success                                               |
| `1`  | assertion or wait condition not met (`expect`/`wait`) |
| `2`  | usage / invalid argument                              |
| `3`  | no active session (run `open`/`run` first)            |
| `4`  | daemon or IPC error                                   |
| `5`  | internal error                                        |

With `--json`, failures also carry a `"kind"` field (`assertion`/`usage`/`no_session`/`internal`).

## Configuration

### Profiles

`tui-test.toml` defines named profiles. Every field is optional:

```toml
[profiles.default]
scrollback = 10000            # rows kept beyond the visible screen

[profiles.default.colors]
background = "#000000"
foreground = "#c0c0c0"
cursor     = "#c0c0c0"
red        = "#800000"        # any of the 16 ANSI slots, by name

[profiles.ci]
scrollback = 500              # other fields use built-in defaults
```

```bash
tui-test open                         # profile "default"
tui-test open --profile ci
tui-test open --config ./other.toml --profile ci
```

By default, `tui-test` checks `./tui-test.toml` first, then `$XDG_CONFIG_HOME/tui-test/tui-test.toml` on Unix, and finally `~/.tui-test/tui-test.toml`. `--config` or `TUI_TEST_CONFIG` skips that search and uses the specified file.

Named profiles do not inherit from `[profiles.default]`; every omitted field uses tui-test's built-in default. `tui-test.toml` affects only the CLI. The libraries accept profile configurations when starting a new session.

### Colors

A terminal grid stores color indices, not display colors. `tui-test` uses the profile's color table both to draw screenshots and to resolve `expect --fg "#rrggbb"`. A color matched by an assertion is the color drawn in a screenshot.

Only the 16 ANSI slots and the three defaults are configurable. Indices 16-255 are the xterm color cube and gray ramp, fixed by the spec, so `--fg 196` means the same thing in every profile.

The default palette is the classic VGA/xterm palette expected by `TERM=xterm-256color`.

## Compatibility

### Supported shells

- bash
- zsh
- fish
- PowerShell (`powershell` and `pwsh`)
- xonsh
- elvish
- nushell
- cmd

### Comparison

| | tui-test | [Tuistory](https://github.com/remorses/tuistory) | [tui-use](https://github.com/onesuper/tui-use) | [terminal-use](https://github.com/flipbit03/terminal-use) |
| --- | --- | --- | --- | --- |
| Language | Rust | TypeScript | TypeScript | Rust |
| Emulator | Alacritty, Ghostty, Rio, or xterm.js, per session | Ghostty (via OpenTUI) | xterm (headless) | Alacritty |
| Shell command tracking | ✅ command boundaries, exit codes, cwd | ❌ | ❌ | ❌ |
| Testing / snapshots | ✅ `expect` text / output / exit-code / snapshot | ✅ snapshots | ❌ | ❌ |
| Color and per-cell attributes | ✅ fg/bg, ANSI-256/hex/RGB, `cells` | ✅ style-filtered text | ❌ plain text with highlights | ✅ via PNG |
| Image screenshots | ✅ SVG | ✅ PNG | ❌ | ✅ PNG |
| Built-in recording | ✅ APNG/GIF/MP4 export and asciinema casts | ❌ | ❌ | ❌ |
| Live monitor view | ✅ | ✅ | ❌ | ✅ |
| Stable exit-code taxonomy for agents | ✅ | ❌ | ❌ | ❌ |
| Python and JavaScript bindings | ✅ | ❌ | ❌ | ❌ |
| Runtime | Native | Node.js | Node.js | Native |
| Platforms | Windows, Linux, and macOS | Windows, Linux, and macOS | Windows, Linux, and macOS | Linux and macOS |

## Debugging

The daemon does not log by default. Start it with `--verbose` to record every byte read from or written to the PTY, along with lifecycle events, in `~/.tui-test/<session>.log`.

## Contributing

This project welcomes contributions and suggestions. Most contributions require you to agree to a Contributor License Agreement (CLA) declaring that you have the right to, and actually do, grant us the rights to use your contribution. For details, visit https://cla.opensource.microsoft.com.

When you submit a pull request, a CLA bot will automatically determine whether you need to provide a CLA and decorate the PR appropriately (e.g., status check, comment). Simply follow the instructions provided by the bot. You will only need to do this once across all repos using our CLA.

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/). For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/) or contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

## Trademarks

This project may contain trademarks or logos for projects, products, or services. Authorized use of Microsoft trademarks or logos is subject to and must follow [Microsoft's Trademark & Brand Guidelines](https://www.microsoft.com/en-us/legal/intellectualproperty/trademarks/usage/general). Use of Microsoft trademarks or logos in modified versions of this project must not cause confusion or imply Microsoft sponsorship. Any use of third-party trademarks or logos are subject to those third-party's policies.
