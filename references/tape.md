# Tape parser and GIF runner

The CLI and Rust `tui_test::tape` module parse and run a limited VHS dialect.
The in-process runner exports one GIF from an owned shell session.
There is no tape binding API.

The grammar reference is VHS v0.12.1, commit
[`24fa2254a9806091e6ee6a980e9f3bcfe0a9ba53`](https://github.com/charmbracelet/vhs/tree/24fa2254a9806091e6ee6a980e9f3bcfe0a9ba53).
The implementation uses the upstream lexer, parser, token definitions, evaluator, and command behavior as references.
It does not promise full VHS compatibility or identical pixels.

## Try the CLI

Warning: Run only trusted tapes. Tapes can execute shell commands; they are not sandboxed.
`check` validates supported syntax and execution requirements. It does not establish that a script is safe.

Save this text as `demo.tape`:

```text
Output demo.gif
Set Columns 40
Set Rows 6
Set Padding 12
Set FontSize 18
Set CursorBlink false
Hide
Type@0 'echo Hello from a tape'
Show
Enter@200ms
Sleep 1
Hide
```

Run these commands from that directory:

```sh
tui-test tape check demo.tape --json
tui-test tape run demo.tape
```

The default shell is `bash` on Unix or `powershell` on Windows.
The example writes `demo.gif` with a 40-column, 6-row terminal and about 1.2 seconds of visible time.
The command reports the actual pixel dimensions, which include padding and font metrics.

From the repository root, you can also run the checked-in fixture:

```sh
tui-test tape check crates/tui-test/tests/fixtures/tape/run.tape
tui-test tape run crates/tui-test/tests/fixtures/tape/run.tape --json
```

This fixture writes `tape-example.gif` in the invocation directory.
Root tape, `Source`, `Require`, and output paths resolve from that directory, not the directory of the containing tape.

Both commands load sources, compile the complete tape, and perform preflight.
`check` never starts a child or creates an output.
`run` uses an owned local session and closes it after success or failure.
EOF closes the shell even if a submitted command still runs.
Tape delays do not wait for command completion.
Neither command uses a daemon, a session registry, project configuration, or `TUI_TEST_SESSION` for session selection.
The child still inherits the process environment, including `TUI_TEST_SESSION`.
Tape `Env` directives change only that child's environment.
On Windows, the child receives an ordinary drive or UNC path for its working directory, not a verbatim path.

Only the global `--json` output flag applies to tape commands.
Explicit `--session`, `--verbose`, `--failure-artifacts`, `--failure-artifact-mode`, `--failure-artifact-recording`, and `--diagnostic-context` flags fail with exit code 2.
The core tape API does not accept an `ExecutionContext`; tape commands do not produce CLI failure bundles.
Use the ordinary session workflow when you need those diagnostics.

Human output includes the output path, shell, terminal grid, pixel dimensions, and source-located warnings.
JSON success has the shape `{"ok":true,"data":SUMMARY}`.
JSON failure has the shape `{"ok":false,"error":ERROR}`.
The [runner result fields](#runner-results-and-errors) describe `SUMMARY` and execution errors.
Parse and compile errors contain `location`, `kind`, and `message`.
CLI option errors contain `kind` and `message`.
JSON diagnostics go to stdout, without additional stderr text.
Clap still handles command-line syntax errors and help in its normal format.
Parse, compile, and preflight failures return exit code 2.
Execution failures use the core error kind: assertion 1, usage 2, no session 3, or internal 5.

## Try the parser

Run the example from the repository root:

```sh
cargo run -p tui-test-rs --example parse_tape
cargo run -p tui-test-rs --example parse_tape -- path/to/demo.tape
```

The example prints a JSON plan. It does not start a child or write a GIF.
The default fixture shows hidden setup, literal text, runtime speed changes, scroll commands, and a final `Hide`.

```rust
use tui_test::tape::{compile, load, parse};

let tape = parse("demo.tape", "Output demo.gif Type 'Hello' Sleep 1")?;
let plan = compile(&tape)?;
assert_eq!(plan.settings().columns, 80);

// This alternative reads the root file and its Source files.
let tape = load("demo.tape", std::env::current_dir()?)?;
let plan = compile(&tape)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Try the GIF runner

Run the example from the repository root:

```sh
cargo run -p tui-test-rs --features recording-font-jetbrains-mono --example run_tape
cargo run -p tui-test-rs --features recording-font-jetbrains-mono --example run_tape -- path/to/demo.tape
```

The default fixture writes `tape-example.gif` in the invocation directory.
It shows hidden setup, typed commands, a trailing visible delay, and a final `Hide`.
The example prints the preflight summary and the completed output path.

```rust
use tui_test::tape::{check, compile, load, run};

let cwd = std::env::current_dir()?;
let plan = compile(&load("demo.tape", &cwd)?)?;
let expected = check(&plan, &cwd)?;
println!("GIF: {}x{}", expected.geometry.width, expected.geometry.height);
let completed = run(&plan, &cwd)?;
assert_eq!(completed.output, expected.output);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Both `check` and `run` require `recording-raster`.
The parser and compiler work without this feature.
The `recording-font-jetbrains-mono` feature also bundles the default font and enables raster support.
Without raster support, both runner functions return a preflight error. Neither reports success.

## API contract

- `parse(path, text)` is pure. It validates the complete syntax and returns a `Tape`.
- `load(path, invocation_cwd)` reads a tape and expands its `Source` commands.
- `resolve_sources(&tape, invocation_cwd)` expands imports from a parsed tape.
- `compile(&tape)` checks the executable subset and returns an owned `Plan`.
- `check(&plan, invocation_cwd)` checks execution requirements without a child or output writes.
- `run(&plan, invocation_cwd)` repeats preflight, starts an owned shell, executes the actions, and exports a GIF.
- No function changes the process environment or directory.
- Only `run` starts a child or writes an output.
- Only `compile` can construct a plan. The plan exposes read-only getters.
- `Tape`, `Plan`, settings, commands, actions, errors, and warnings implement `Serialize`.

`Plan` exposes the output, initial settings, shell location, environment directives, requirements, actions, initial visibility, and warnings.
Environment directives retain source order. The last duplicate key wins in the child environment.
Runtime `TypingSpeed` changes become explicit delays on subsequent actions.
The initial settings remain unchanged.

Locations contain a path, line, and column. Lines and Unicode scalar columns start at one.
Import locations retain the parent `Source` location.
Errors and warnings display `file:line:column`, with import context when applicable.
`ErrorKind` distinguishes syntax, invalid values, unsupported features, and source errors.

### Runner results and errors

Both runner functions return a serializable `Summary` with these fields:

- `output`: the resolved GIF path.
- `shell`: the executable path under the effective child environment.
- `geometry`: terminal `columns` and `rows`, plus actual GIF `width` and `height` in pixels.
- `warnings`: the compiler warnings, with their original locations.

`RunnerError` contains `location`, `phase`, `kind`, `message`, and `cleanup_errors`.
`RunnerPhase::Preflight` identifies validation failures. These use the core `ErrorKind::Usage`.
`RunnerPhase::Execution` identifies child, terminal, or export failures. These retain the core error kind.
Cleanup errors do not replace the primary error.
Settings errors identify the effective setting directive when available.

Preflight checks the complete plan before any child or output file exists.
It checks executable requirements, generated key tokens, fonts, style limits, GIF dimensions, and known timeline limits.
The plan remains unchanged.

The invocation directory and output parent must already exist.
The runner does not create missing output directories.
An existing GIF can be replaced after a successful export.
Failed validation never replaces it.
Output symlinks, directories, and read-only files are errors.
Existing `.tui-test.cast` or `.tui-test.tmp` work files are errors, not files that the runner can discard.
Filesystem changes after preflight can still cause execution failures.

The core stages the GIF before replacement.
If export fails, it retains the recovery cast and includes its path in the error.
The runner does not delete that cast or user files.
Recovery casts are diagnostic records and can contain hidden output. Do not treat them as redacted GIFs.

### Child environment and shell startup

The child inherits the process environment.
All `Env` directives apply in source order before shell startup. The last duplicate key wins.
On Windows, environment keys compare without case sensitivity.
The core currently requires UTF-8 environment keys, values, and paths. Preflight reports unsupported values.

`Require` checks executable files, not shell builtins.
The shell and requirements use the effective child `PATH` and Windows `PATHEXT`.
Relative search directories and explicit executable paths use the invocation directory.
A Windows executable with a matching extension does not get a second extension.

The runner uses `Operation::Run`, not `Open`.
It does not load project configuration, install shell integration, or use a daemon or global session registry.
It closes its child after success or failure.
It waits up to five seconds for the controlled `> ` prompt before frame capture.
It does not use `WaitReady`, which requires shell integration.

| Shell | Startup controls |
| --- | --- |
| bash | `--noprofile --norc -i`; constant prompt; history disabled |
| zsh | `-d -f -i`; user and normal global startup files disabled; history disabled |
| fish | `--no-config --private -i`; fixed prompt functions; no greeting |
| powershell, pwsh | `-NoLogo -NoProfile -NoExit`; fixed prompt; PSReadLine history writes disabled |
| cmd | `/D /Q /K`; AutoRun disabled; fixed `prompt $G$S` command |

Zsh can still read its mandatory system `zshenv`. The shell provides no flag to disable that file.
No shell argument contains a tape-supplied command string.
`Type` supplies terminal input, not a startup argument.

Prompt and history controls deliberately override conflicting `Env` directives.
These controls include `PS1`, `PS2`, `PROMPT`, `RPROMPT`, `PROMPT_COMMAND`, `HISTFILE`, `HISTSIZE`, and `SAVEHIST`.
The runner also overrides `BASH_ENV`, `ENV`, and `INPUTRC` to prevent user startup and Readline configuration.
Other variables, including `PATH`, retain their effective tape values.

## Grammar

- Command and setting names are case-sensitive.
- Spaces, tabs, CR, and LF are whitespace. A newline is not a statement separator.
- `#` starts a comment outside a quoted value.
- Single quotes, double quotes, and backticks contain literal text.
- Quotes do not interpret JSON escapes or shell escapes.
- Quoted text must end before a newline.
- `/pattern/` contains a regular expression. An odd backslash count escapes its closing delimiter.
- Regex escapes remain in the pattern. The parser checks the pattern with Rust `regex`.
- Bare words use the upstream ASCII word rules. Quote absolute paths and special characters.
- `Type` and `Copy` join adjacent string tokens with one space, even across a newline.
- A comment ends an adjacent string group.

For example, `Type 'a\n' "b"` types a literal backslash and `n`, then a space and `b`.
Quote words that are keywords when you need literal text.
An unknown bare word after `Type` is text, not a separate command.

### Durations and repetitions

`Sleep`, `@duration`, and `WaitTimeout` accept bare seconds, `ms`, `s`, and `m`.
Decimal durations use checked arithmetic with an upper limit of `i64::MAX` nanoseconds.
Fractions below one nanosecond are discarded.
Negative durations, malformed numbers, and overflow are errors.

`Set TypingSpeed` follows the upstream unit quirks:

| Input | Result |
| --- | --- |
| `Set TypingSpeed 50` | 50 seconds |
| `Set TypingSpeed 50ms` | 50 milliseconds |
| `Set TypingSpeed .1s` | 100 milliseconds |
| `Set TypingSpeed 1m` | Error; use `60s` |
| `Set TypingSpeed '1m'` | 1 millisecond; upstream appends `s` |
| `Set TypingSpeed '50ms'` | Error; upstream appends another `s` |

`Type@50ms 'text'` overrides the current delay.
`Enter@100ms 3` repeats a key three times.
Counts must be nonnegative integers within `u64`. Zero repeats do no work.
`ScrollUp` and `ScrollDown` use the same delay and repeat syntax.

The plan preserves each Unicode scalar in `Type`.
The runner delays after each scalar, including the last scalar.
Ordinary keys and scroll commands use the same delay after every step.
Each scroll step moves the viewport by one row.
`ScrollUp` moves toward older output with `ScrollTarget::Lines(1)`.
`ScrollDown` moves toward live output with `ScrollTarget::Lines(-1)`.
Neither command sends mouse input to the child.

### Keys and modifiers

Ordinary keys are `Backspace`, `Delete`, `Insert`, `Enter`, `Escape`, `Tab`, `Space`,
`Up`, `Down`, `Left`, `Right`, `PageUp`, and `PageDown`.
The pinned parser does not accept standalone `Home` or `End`.

`Ctrl+Alt+Shift+x+c` holds the listed modifiers for both keys.
Only `Ctrl` starts a modifier chain. All modifiers must precede the keys.
Ctrl permits `Enter`, `Space`, `Backspace`, arrows, and a single literal character.
The punctuation keys `-`, `@`, `[`, `]`, `^`, and `\` can appear without quotes.
Quote digits and reserved letters, as in `Ctrl+'1'` or `Ctrl+'s'`.

`Alt` and `Shift` accept one character, `Enter`, `Tab`, `[` or `]`.
Unlike upstream fallback behavior, unknown multicharacter key names are errors.
Quoted Unicode scalar keys are accepted.
Modifier commands do not accept repeat counts or `@duration`.
The runner sends every key in the chain through the core mode-aware key operation.
It maps a quoted modified `+` to the US `Shift+=` key and preserves the other modifiers.

## Supported executable subset

A plan requires exactly one explicit `.gif` output in its initial section.
Other formats, multiple outputs, and late outputs fail validation.
Output paths must end in a filename, not a path separator or `.`.
VHS permits late output changes; this subset rejects them instead of moving commands.
The output path remains relative to the invocation directory.

The supported settings are:

| Setting | Initial value | Validation |
| --- | --- | --- |
| `Shell` | `bash` on Unix; `powershell` on Windows | Controlled name only |
| `Rows` | 24 | Positive `u16`; at most 32767 on Windows |
| `Columns` | 80 | Positive `u16`; at most 32767 on Windows |
| `FontFamily` | `JetBrains Mono` | Nonempty string without NUL |
| `FontSize` | 22 | Finite positive `f32` |
| `Framerate` | 50 | Integer from 1 to 255 |
| `TypingSpeed` | 50ms | Nonnegative checked duration |
| `PlaybackSpeed` | 1.0 | Finite positive `f64` |
| `Padding` | 60 | Nonnegative `u32` integer |
| `CursorBlink` | `true` | Unquoted `true` or `false` |

Supported shell names are `bash`, `zsh`, and `fish` on Unix.
Supported Windows names are `powershell`, `pwsh`, and `cmd`.
Shell paths, argument lists, aliases, and other shells are unsupported.
Shell names are not expanded.

Supported commands are `Type`, ordinary keys, modifiers, `Sleep`, `Hide`, `Show`,
`ScrollUp`, `ScrollDown`, `Env`, `Require`, and `Source`.
`Env` requires a nonempty key without `=` or NUL. Its value must be one literal string without NUL.

### Settings phases

VHS hoists all `Env` and `Set Shell` directives before it starts a child.
The plan preserves this behavior without process environment changes.
The runner checks shell and `Require` availability under the effective child environment, including `PATH`.

The initial section contains only `Set`, `Output`, and `Require` commands.
Other commands end that section after `Source` expansion.
In particular, `Env` ends the section even though its effect is hoisted.

After the initial section:

- `Set Shell` still applies through hoisting.
- `Set TypingSpeed` changes subsequent action delays.
- Other supported settings produce a located warning and have no effect.
- Unsupported settings remain errors.
- `Require` fails with an initial-section error instead of silently skipping the check.
- `Output` fails with a late-output error.

### Source files

`Source` requires a `.tape` path relative to the invocation directory, not the includer.
Absolute paths must be quoted.
Imported output commands are discarded before compilation.
Their syntax must still be valid.

Nested imports fail before any nested file read.
Empty files and files with no commands fail.
Every imported command keeps its file location and parent import location.
Source errors and warnings retain that context.

### Visibility and output style

The compiler rejects tapes with no visible work.
A visible nonempty `Type`, positive repeat count, positive `Sleep`, modifier action, or hidden-to-visible transition supplies visible work.
Repeated `Hide` and `Show` commands are no-ops.
An initial `Hide` sets `initially_visible()` to false and does not remain as an action.
The runner delays frame capture until the first `Show`.
A final `Hide` after visible work is valid.
The runner never resumes capture at EOF.
Visible `Hide` pauses frame capture. The next `Show` resumes it from the live terminal.
Hidden elapsed time does not extend the GIF timeline.
Content still on screen when `Show` executes becomes visible. `Hide` does not erase the terminal.

The native grid differs from VHS default pixel geometry. Every plan reports this warning.
`CursorBlink true` permits cursor animation but does not force it.
An application can select a steady cursor.
Plans with this setting report the difference.
The core cursor defaults remain enabled with a 500ms interval.

The runner disables borders, shadows, title bars, traffic lights, and canvas padding.
It uses the tape padding as content padding.
It uses recording zoom `0.5` for native 1x raster output because the core animation scale multiplies zoom by two.
It disables idle compression with `idle_time_limit: None`.
Visible quiet time and trailing `Sleep` remain in the GIF, divided by `PlaybackSpeed`.
Delays use real elapsed time. Host scheduling and input processing can extend visible time beyond the requested delays.
The runner adds no final hold. GIF centisecond quantization and frame sampling still apply.

Preflight uses the same style validation and `GridRenderer` geometry as export.
At least one named font in `FontFamily` must exist in the bundled or system catalog.
CSS generic families retain the core fallback behavior.
Native font metrics and glyph fallback can differ between hosts.

## Runner validation

The runner tests decode GIF frames and delays rather than only checking file sizes.
They cover hidden frames, quiet time, playback speed, Unicode input, modifiers, viewport movement, and child cleanup.
Timing checks use measured visible intervals, not fixed limits on host scheduling delays.
Tests create unique directories under the worktree's `target` directory and remove their artifacts.

```sh
cargo test -p tui-test-rs --features recording-font-jetbrains-mono tape::
cargo test -p tui-test-rs --features recording-font-jetbrains-mono --test tape_runner
cargo test -p tui-test-rs --no-default-features tape::
```

Unix tests exercise installed bash, zsh, and fish executables.
Unix raw-PTY fixtures verify exact input bytes and scroll behavior.
Windows-native tests exercise installed PowerShell, pwsh, and cmd executables.
They also check case-insensitive environment overrides and explicit `.EXE` lookup with `PATHEXT`.
Run those tests on Windows; cross-compilation alone does not verify ConPTY behavior.

## Recognized but unsupported

The parser gives these commands a typed shape:

- `Wait[+Line|+Screen][@duration] [/pattern/]`
- `Screenshot path.png`
- `Copy 'text'`
- `Paste`

The parser also recognizes all remaining settings:
`Height`, `Width`, `LetterSpacing`, `LineHeight`, `Theme`, `LoopOffset`, `MarginFill`,
`Margin`, `WindowBar`, `WindowBarSize`, `BorderRadius`, `WaitTimeout`, and `WaitPattern`.
`Theme` accepts a name or a flat JSON object, as in the pinned grammar.

Compilation rejects each unsupported command or setting with its source location.
Malformed syntax and invalid values fail earlier in the parser.
Unknown commands and settings never become no-ops.
