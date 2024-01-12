import { test, expect, Shell } from "@microsoft/tact-test";

test.use({ shell: Shell.Cmd });

test("", async ({ terminal }) => {
  terminal.write("");
  terminal.resize(80, 15);

  await expect(terminal).toHaveValue("> ");
  expect(terminal.cursor().x).toBe(59);
});

test("", async ({ terminal }) => {
  terminal.write("");
  terminal.resize(80, 15);

  await expect(terminal).toHaveValue("> ");
  // expect(terminal.cursor().x).toBe(59);
});
