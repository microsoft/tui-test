import { TuiTest } from "../dist/index.js";

const terminal = TuiTest.ephemeral(undefined, { backend: "alacritty" });
try {
  await terminal.run(process.execPath, ["-e",
    "for(let i=0;i<40;i++)console.log('history-'+i);process.stdin.resume()",
  ], { rows: 5, cols: 40 });
  await terminal.getByText("history-39").expect();
  const full = await terminal.text({ full: true });
  console.log("Live screen:\n" + await terminal.text());
  await terminal.scrollToTop();
  console.log("History viewport:", await terminal.getViewport());
  console.log("Oldest screen:\n" + await terminal.text());
  console.log("Full text unchanged:", full === await terminal.text({ full: true }));
  await terminal.scroll(-2);
  console.log("Two rows toward live:", await terminal.getViewport());
  await terminal.scrollToBottom();
  console.log("Live viewport:", await terminal.getViewport());
} finally {
  await terminal.closeQuiet();
}
