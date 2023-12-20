import pty, { IPty, IEvent } from "node-pty";
import xterm from "xterm-headless";
import process from "node:process";
import { Shell, shellTarget } from "./shell.js";

type TerminalOptions = {
  env?: { [key: string]: string | undefined };
  rows: number;
  cols: number;
  shell: Shell;
  shellArgs?: string[];
};

export const spawn = async (options: TerminalOptions): Promise<Terminal> => {
  return new Terminal(await shellTarget(options.shell), options.shellArgs ?? [], options.rows, options.cols, options.env);
};

export class Terminal {
  readonly #pty: IPty;
  readonly #term: xterm.Terminal;
  readonly onExit: IEvent<{ exitCode: number; signal?: number }>;

  constructor(shellTarget: string, shellArgs: string[], private _rows: number, private _cols: number, env?: { [key: string]: string | undefined }) {
    this.#pty = pty.spawn(shellTarget, shellArgs ?? [], {
      name: "xterm-256color",
      cols: this._cols,
      rows: this._rows,
      cwd: process.cwd(),
      env,
    });
    this.#term = new xterm.Terminal({ allowProposedApi: true, rows: this._rows, cols: this._cols });

    this.#pty.onData((data) => {
      this.#term.write(data);
    });
    this.onExit = this.#pty.onExit;
  }

  resize(columns: number, rows: number) {
    this._cols = columns;
    this._rows = rows;
    this.#pty.resize(columns, rows);
    this.#term.resize(columns, rows);
  }

  write(data: string): void {
    this.#pty.write(data);
  }

  getViewableBuffer(): string[][] {
    const lines: string[][] = [];
    for (let y = this.#term.buffer.active.baseY; y < this.#term.buffer.active.length; y++) {
      const termLine = this.#term.buffer.active.getLine(y);
      const line: string[] = [];
      let cell = termLine?.getCell(0);
      for (let x = 0; x < this.#term.cols; x++) {
        cell = termLine?.getCell(x, cell);
        const rawChars = cell?.getChars() ?? "";
        const chars = rawChars === "" ? " " : rawChars;
        line.push(chars);
      }
      lines.push(line);
    }
    return lines;
  }

  cursor() {
    return {
      x: this.#term.buffer.active.cursorX,
      y: this.#term.buffer.active.cursorY,
      baseY: this.#term.buffer.active.baseY,
    };
  }

  kill() {
    process.kill(this.#pty.pid, 9);
  }
}
