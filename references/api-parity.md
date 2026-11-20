# Public API parity

The parity contract covers terminal sessions, not identical method names or memory layouts.
Rust defines the operations in `crates/tui-test/src/api.rs`.
The CLI, JavaScript, Python, and Go must expose their behavior, inputs, and results.

## Contract

| Area | Rust operations and inputs |
| --- | --- |
| Lifecycle | `Open`, `Run`, `Restart`, `Close` |
| Trace outcome | `FinishTrace`, including a failed outcome without closing the session |
| Screen | `State`, `Text`, `PackedScreen`, `Cells` |
| Command state | `GetCommand`, `GetOutput`, `GetExitCode`, `GetCwd` |
| Terminal state | `GetCursor`, `GetModes`, `GetColors`, `GetSize`, `GetViewport`, `GetTitle` |
| Terminal events | `GetClipboard`, `GetBellCount`, `GetBellEvents` |
| Application progress | `GetProgressBar`, `WaitProgressBar`, `ExpectProgressBar` |
| Program status | `GetProgramStatuses`, `GetProgramStatus`, `WaitProgramStatus`, `ExpectProgramStatus` |
| Input | `Write`, `Submit`, `Key` with every `KeyAction`, and `Mouse` with every `MouseAction` |
| Process and viewport | `Resize`, `Scroll`, `Signal` |
| Event waits | `WaitTitle`, `WaitClipboard`, `WaitClipboardMatch`, `WaitBell` |
| Lifecycle waits | `WaitIdle`, `WaitCommand`, `WaitExit`, `WaitReady` |
| Locator results | `FindLocator`, `ResolveLocator` |
| Locator actions | `WaitLocator`, `ClickLocator`, `HighlightLocator` |
| Assertions | `ExpectTitle`, `ExpectExitCode`, `ExpectMode`, `ExpectColors`, `ExpectCursor`, `ExpectOutput`, `ExpectBellCount` |
| Snapshots | `Snapshot`, including update, style, title, and directory options |
| Capture | `Screenshot`, `StartRecording`, `PauseRecording`, `ResumeRecording`, `StopRecording` |

The operation list alone is not sufficient. The following input and output contracts also apply:

- Session creation preserves backend, shell or program, arguments, dimensions, directory, environment, readiness, restart, timeouts, profile, recording, and rendering settings.
- Locators preserve text, style, link, intersection, union, filter, occurrence, parent query, relative direction, and full-screen selection.
- Text scopes preserve both anchors, their regex flags, and their occurrence selections.
- A query style filter applies to the whole match. A style-selector intersection does not replace this filter.
- Diagnostics preserve caller context, artifact settings, trace settings, retention, and failure evidence.
- Results preserve optional values, terminal attributes, match spans, palette entries, event sequence numbers, and snapshot outcomes.
- Ghostty and Rio expose native OSC 9;4 progress. Other backends return usage errors and null snapshot fields.
- Progress matches compare supplied fields and validate percentages from 0 to 100.
- Program status uses native Ghostty or Rio callbacks. Other backends return usage errors and null snapshot fields.
- Status lists use stable ID order. An empty ID selects the root; an absent record never matches.
- Status filters compare supplied fields exactly. Omitted IDs select the root, not every record.
- Packed screens preserve dimensions, UTF-8 bytes, trailing spaces, and blank rows. Byte offsets are not terminal columns.

## Equivalent interfaces

Language conventions may differ without changing the contract.
For example, Go uses `time.Duration`; the Rust API uses milliseconds.
The CLI prints structured results with `--json`; libraries return typed values.
The CLI can read profile and rendering settings from a config file instead of separate flags for each property.

Session names and automatic recording access follow each host's session model.
CLI sessions belong to a daemon. Library sessions belong to the current process.
They do not share a registry.

## Locator and diagnostic interfaces

| Capability | JavaScript | Python | Go | CLI |
| --- | --- | --- | --- | --- |
| Text anchors | `TextSelectorOptions.scope` | `scope=TextScope(...)` | `TextSelectorOptions.Scope` | `--after-text`, `--before-text`, or a query file |
| Whole-match style | `withStyle()` | `with_style()` | `WithStyle()` | Text style flags or a query's `style` field |
| Relative query | `relativeTo()` | `relative_to()` | `RelativeTo()` | A query's `within` and `direction` fields |
| Selected match | `location()` | `location()` | `Location()` | `resolve text` or `resolve query` |
| Highlight results | `highlightMatches()` | `highlight_matches()` | `HighlightMatches()` | `highlight ... --json` |
| Packed screen | `packedScreen()` | `packed_screen()` | `PackedScreen()` | `packed-screen --json` |
| Trace outcome | `finishTrace()` | `finish_trace()` | `FinishTrace()` | `trace finish passed\|failed` |
| Session interruption | `interrupt()` | `interrupt()` | `Interrupt()` | `interrupt` |
| Progress result | `getProgressBar()` | `get_progress_bar()` | `GetProgressBar()` | `get progress-bar` |
| Progress wait | `waitProgressBar()` | `wait_progress_bar()` | `WaitProgressBar()` | `wait progress-bar` |
| Progress assertion | `expectProgressBar()` | `expect_progress_bar()` | `ExpectProgressBar()` | `expect progress-bar` |
| Program status list | `getProgramStatuses()` | `get_program_statuses()` | `GetProgramStatuses()` | `get program-status` |
| Program status record | `getProgramStatus(id)` | `get_program_status(id)` | `GetProgramStatus(id)` | `get program-status --id ID` |
| Program status wait | `waitProgramStatus()` | `wait_program_status()` | `WaitProgramStatus()` | `wait program-status` |
| Program status assertion | `expectProgramStatus()` | `expect_program_status()` | `ExpectProgramStatus()` | `expect program-status` |
| Execution context | `context: ExecutionContext` | `context=ExecutionContext(...)` | `Context: *ExecutionContext` | `--context-file PATH` |

The CLI accepts nested Rust `LocatorQuery` JSON through `--query` or `--query-file`.
Library locator builders use the shared expression transport internally.
Both paths use the same Rust evaluator.

Constructor contexts establish defaults. A per-spawn context applies to that operation without changing the constructor options.
Automatic recording modes are `disabled`, `on-failure`, and `always`.

Runnable examples:

- [JavaScript](../bindings/js/examples/parity.mjs)
- [Python](../bindings/python/examples/parity.py)
- [Go](../bindings/go/examples/parity/main.go)
- [CLI](cli.md)

## Behavioral requirements

Omitted options retain their defaults. Explicit false and zero remain explicit.
Operations do not mutate caller-owned options or locator expressions.
Interruption cancels waits and terminates the child without removing the session.
All surfaces use the core locator evaluator and preserve its errors and selection rules.
Composition nodes reject parent scopes and whole-match style filters, as the Rust evaluator requires.
Apply those fields to the supported leaf queries instead.

Pause and resume support APNG, GIF, and MP4 only.
Asciicast rejects these operations without changing its stream.
Automatic diagnostic casts and raster recovery casts remain continuous.

## Boundaries

This contract does not expose Rust implementation interfaces in other languages.
Those include emulator traits, standalone rendering components, logger implementations, monitor transport, and Rust synchronization primitives.
It does not require the CLI to expose pointers, borrowed memory, or process-local callbacks.

Backend and operating-system limitations apply equally to every surface.
Transport validation and language-specific resource management remain necessary.
Neither constitutes an alternative implementation of terminal behavior.
