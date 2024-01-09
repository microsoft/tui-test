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

type CellShift = {
  bgColorMode?: number;
  bgColor?: number;
  fgColorMode?: number;
  fgColor?: number;
  blink?: number;
  bold?: number;
  dim?: number;
  inverse?: number;
  invisible?: number;
  italic?: number;
  overline?: number;
  strike?: number;
  underline?: number;
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

  getBuffer(): string[][] {
    return this._getBuffer(0, this.#term.buffer.active.length);
  }

  getViewableBuffer(): string[][] {
    return this._getBuffer(this.#term.buffer.active.baseY, this.#term.buffer.active.length);
  }

  private _getBuffer(startY: number, endY: number): string[][] {
    const lines: string[][] = [];
    for (let y = startY; y < endY; y++) {
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

  private _shift(baseCell: xterm.IBufferCell | undefined, targetCell: xterm.IBufferCell | undefined): CellShift {
    const result: CellShift = {};
    if (!(baseCell?.getBgColorMode() == targetCell?.getBgColorMode() && baseCell?.getBgColor() == targetCell?.getBgColor())) {
      result.bgColorMode = targetCell?.getBgColorMode();
      result.bgColor = targetCell?.getBgColor();
    }
    if (!(baseCell?.getFgColorMode() == targetCell?.getFgColorMode() && baseCell?.getFgColor() == targetCell?.getFgColor())) {
      result.fgColorMode = targetCell?.getFgColorMode();
      result.fgColor = targetCell?.getFgColor();
    }
    if (baseCell?.isBlink() !== targetCell?.isBlink()) {
      result.blink = targetCell?.isBlink();
    }
    if (baseCell?.isBold() !== targetCell?.isBold()) {
      result.bold = targetCell?.isBold();
    }
    if (baseCell?.isDim() !== targetCell?.isDim()) {
      result.dim = targetCell?.isDim();
    }
    if (baseCell?.isInverse() !== targetCell?.isInverse()) {
      result.inverse = targetCell?.isInverse();
    }
    if (baseCell?.isInvisible() !== targetCell?.isInvisible()) {
      result.invisible = targetCell?.isInvisible();
    }
    if (baseCell?.isItalic() !== targetCell?.isItalic()) {
      result.italic = targetCell?.isItalic();
    }
    if (baseCell?.isOverline() !== targetCell?.isOverline()) {
      result.overline = targetCell?.isOverline();
    }
    if (baseCell?.isStrikethrough() !== targetCell?.isStrikethrough()) {
      result.strike = targetCell?.isStrikethrough();
    }
    if (baseCell?.isUnderline() !== targetCell?.isUnderline()) {
      result.underline = targetCell?.isUnderline();
    }
    return result;
  }

  serialize(): { view: string; shifts: Map<string, CellShift> } {
    const shifts = new Map<string, CellShift>();
    const lines = [];

    const empty = (o: object) => Object.keys(o).length === 0;
    let prevCell = undefined;
    for (let y = this.#term.buffer.active.baseY; y < this.#term.buffer.active.length; y++) {
      const line = this.#term.buffer.active.getLine(y);
      const lineView = [];
      if (line == null) continue;
      for (let x = 0; x < line.length; x++) {
        const cell = line.getCell(x);
        const chars = cell?.getChars() ?? "";
        lineView.push(chars.length === 0 ? " " : chars);
        const shift = this._shift(prevCell, cell);
        if (!empty(shift)) {
          shifts.set(`${x},${y}`, shift);
        }
        prevCell = cell;
      }
      lines.push(lineView.join(""));
    }
    const view = this._box(lines.join("\n"), this.#term.cols);
    return { view, shifts };
  }

  private _box(view: string, width: number) {
    const top = "╭" + "─".repeat(width) + "╮";
    const bottom = "╰" + "─".repeat(width) + "╯";
    return [top, ...view.split("\n").map((line) => "│" + line + "│"), bottom].join("\n");
  }

  kill() {
    process.kill(this.#pty.pid, 9);
  }
}
