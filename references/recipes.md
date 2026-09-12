# Recipes

[Back to the skill](../SKILL.md)

## Run a command

```sh
tui-test open
tui-test submit "npm test"
tui-test wait command
tui-test expect exit-code 0
```

## Drive an app

```sh
tui-test run my-app
tui-test expect text "Ready"
tui-test click text "Continue"
tui-test expect text "Done"
```

## Match relative text

```sh
tui-test click text "Save" --after-text "Settings" --match unique
```

```python
await (
    terminal
    .get_by_text("Settings")
    .get_by_text("Save", direction="after")
    .unique()
    .click()
)
```

```js
await terminal
  .getByText("Settings")
  .getByText("Save", { direction: "after" })
  .unique()
  .click();
```

## Wait for loading

```python
loading = terminal.get_by_text("Loading")
await loading.wait()
await loading.wait(state="hidden")
await terminal.get_by_text("Ready").expect()
```

## Click with modifiers

```sh
tui-test click text "Open" --button right --ctrl
```

```python
await terminal.get_by_text("Open").click(button="right", ctrl=True)
```

```js
await terminal.getByText("Open").click({ button: "right", ctrl: true });
```

## Wait for clipboard

```python
await terminal.wait_clipboard("copied")
```

```js
await terminal.waitClipboard(/copied/i);
```

## Keep failure artifacts

```sh
tui-test --failure-artifacts artifacts/failures \
  --failure-artifact-recording \
  expect text "Ready" --timeout 5000
```

```python
terminal = TuiTest(
    artifacts={
        "dir": "artifacts/failures",
        "on_failure": "bundle",
        "include_recording": True,
    },
    recording={"mode": "on-failure", "directory": "artifacts"},
)
```

```js
const terminal = new TuiTest("test", {
  artifacts: {
    dir: "artifacts/failures",
    onFailure: "bundle",
    includeRecording: true,
  },
  recording: { mode: "on-failure", directory: "artifacts" },
});
```

Agents should read `failure.md` first and use `failure.json` / `timeline.json` for exact structured evidence. Users can open `failure.html` directly from disk, select actions or filmstrip frames, read explicit expected/observed values, and click cells for style and mismatch metadata. The HTML embeds the available evidence for offline preview/download, including the pinned terminal text/SVG and an explicitly requested cast; it can be distributed alone. Review terminal evidence before uploading it.
