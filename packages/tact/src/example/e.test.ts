import { test, expect, Shell } from "../test/test.js";

test.use({ shell: Shell.Cmd });

test("", async ({ terminal }) => {
  terminal.write("");
  terminal.resize(80, 30);

  await expect(terminal).toHaveValue("> ");
  await expect(terminal.cursor().x).toBe(10);
});
