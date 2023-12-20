import { test, expect, Shell } from "../testing/test.js";

test.use({ shell: Shell.Cmd });

test("", async ({ terminal }) => {
  terminal.write("");
  terminal.resize(80, 30);

  await test.wait(100);

  await expect(terminal).toHaveValue("tomato");
  await expect(terminal).toMatchSnapshot();
  await expect(terminal.cursor().x).toBe(10);
});
