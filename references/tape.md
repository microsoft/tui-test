# Tape parser

The Rust `tui_test::tape` module parses and validates a limited VHS dialect.
This layer does not execute tapes or export recordings.
It does not promise full VHS compatibility or identical pixels.

The grammar reference is VHS v0.12.1, commit
[`24fa2254a9806091e6ee6a980e9f3bcfe0a9ba53`](https://github.com/charmbracelet/vhs/tree/24fa2254a9806091e6ee6a980e9f3bcfe0a9ba53).

## Example

Run the parser example from the repository root:

```sh
cargo run -p tui-test-rs --example parse_tape
cargo run -p tui-test-rs --example parse_tape -- path/to/demo.tape
```

The example prints a JSON plan. It does not start a child or write a GIF.
The default fixture includes hidden setup, literal text, speed changes, scroll commands, and a final `Hide`.

## API

- `parse(path, text)` validates syntax without I/O.
- `load(path, invocation_cwd)` reads a tape and expands its `Source` commands.
- `resolve_sources(&tape, invocation_cwd)` expands imports from a parsed tape.
- `compile(&tape)` checks the supported subset and creates an immutable `Plan`.

Locations include a path, line, Unicode scalar column, and import context.
Lines and columns start at one.
Errors distinguish syntax, invalid values, unsupported features, and source failures.
Syntax, plans, errors, and warnings support serialization.

## Supported subset

A plan requires one explicit `.gif` output in its initial section.
The compiler rejects directory endings, multiple outputs, late outputs, and other formats.
It does not check the filesystem or executable availability.

Supported settings are `Shell`, `Rows`, `Columns`, `FontFamily`, `FontSize`,
`Framerate`, `TypingSpeed`, `PlaybackSpeed`, `Padding`, and `CursorBlink`.
Supported actions are `Type`, ordinary keys, modifiers, `Sleep`, `Hide`, `Show`,
`ScrollUp`, and `ScrollDown`.
The parser also supports `Env`, `Require`, and `Source`.

The plan preserves all environment directives and the effective shell before execution.
Runtime `TypingSpeed` changes become explicit action delays.
Other supported late settings produce warnings and have no effect.
Late `Require` and `Output` commands fail compilation.

Source paths resolve from the invocation directory, not the including file.
Imported output commands are discarded.
Nested imports and empty source files fail with located errors.

Repeated `Hide` and `Show` commands are no-ops.
An initial `Hide` sets the initial visibility.
A final `Hide` is valid after visible work.
The compiler rejects tapes with no visible work.

Quotes contain literal text; they do not interpret shell or JSON escapes.
Newlines are whitespace, not statement separators.
Durations use checked arithmetic.
The parser preserves the pinned upstream grammar's duration and key restrictions.
The tests cover its edge cases.

## Unsupported features

The parser recognizes `Wait`, `Screenshot`, `Copy`, `Paste`, and additional VHS settings.
Compilation rejects these features with their source locations.
Unknown commands and settings never become silent no-ops.
The native grid and cursor-blink permission differ from VHS; plans report these differences.

## Validation

```sh
cargo test -p tui-test-rs --no-default-features tape::
```
