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

```python
terminal = TuiTest(
    artifacts={"dir": "artifacts"},
    recording={"mode": "on-failure", "directory": "artifacts"},
)
```

```js
const terminal = new TuiTest("test", {
  artifacts: { dir: "artifacts" },
  recording: { mode: "on-failure", directory: "artifacts" },
});
```
