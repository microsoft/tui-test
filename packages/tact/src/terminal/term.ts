// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import pty, { IPty, IEvent } from "@homebridge/node-pty-prebuilt-multiarch";
import xterm from "xterm-headless";
import process from "node:process";

import ansi from "./ansi.js";

import { Shell, shellLaunch, shellEnv } from "./shell.js";

type TerminalOptions = {
  env?: { [key: string]: string | undefined };
  rows: number;
  cols: number;
  shell: Shell;
  shellArgs?: string[];
};

type CursorPosition = {
  /**
   * The x position of the cursor. This ranges between 0 (left side) and Terminal.cols (after last cell of the row).
   */
  x: number;
  /**
   * The y position of the cursor. This ranges between 0 (when the cursor is at baseY) and Terminal.rows - 1 (when the cursor is on the last row).
   */
  y: number;
  /**
   * The line within the buffer where the top of the bottom page is (when fully scrolled down).
   */
  baseY: number;
};

export const spawn = async (options: TerminalOptions): Promise<Terminal> => {
  const { shellTarget, shellArgs } = await shellLaunch(options.shell);
  return new Terminal(shellTarget, options.shellArgs ?? shellArgs ?? [], options.rows, options.cols, { ...shellEnv(options.shell), ...options.env });
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

  constructor(
    shellTarget: string,
    shellArgs: string[],
    private _rows: number,
    private _cols: number,
    env?: { [key: string]: string | undefined },
  ) {
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

  /**
   * Change the size of the terminal
   *
   * @param columns Count of column cells
   * @param rows Count of row cells
   */
  resize(columns: number, rows: number) {
    this._cols = columns;
    this._rows = rows;
    this.#pty.resize(columns, rows);
    this.#term.resize(columns, rows);
  }

  /**
   * Write the provided data through to the shell
   *
   * @param data Data to write to the shell
   */
  write(data: string): void {
    this.#pty.write(data);
  }

  /**
   * Press up arrow key a specific amount of times.
   *
   * @param count Count of cells to move up. Default is `1`.
   */
  keyUp(count?: number | undefined): void {
    this.#pty.write(ansi.keyUp.repeat(count ?? 1));
  }

  /**
   * Press down arrow key a specific amount of times.
   *
   * @param count Count of cells to move down. Default is `1`.
   */
  keyDown(count?: number | undefined): void {
    this.#pty.write(ansi.keyDown.repeat(count ?? 1));
  }

  /**
   * Press left arrow key a specific amount of times.
   *
   * @param count Count of cells to move left. Default is `1`.
   */
  keyLeft(count?: number | undefined): void {
    this.#pty.write(ansi.keyLeft.repeat(count ?? 1));
  }

  /**
   * Press right arrow key a specific amount of times.
   *
   * @param count Count of cells to move right. Default is `1`.
   */
  keyRight(count?: number | undefined): void {
    this.#pty.write(ansi.keyRight.repeat(count ?? 1));
  }

  /**
   * Press escape key a specific amount of times.
   *
   * @param count Count of key presses. Default is `1`.
   */
  keyEscape(count?: number | undefined): void {
    this.#pty.write(ansi.ESC.repeat(count ?? 1));
  }

  /**
   * Press delete key a specific amount of times.
   *
   * @param count Count of key presses. Default is `1`.
   */
  keyDelete(count?: number | undefined): void {
    this.#pty.write(ansi.keyDelete.repeat(count ?? 1));
  }

  /**
   * Press backspace key a specific amount of times.
   *
   * @param count Count of key presses. Default is `1`.
   */
  keyBackspace(count?: number | undefined): void {
    this.#pty.write(ansi.keyBackspace.repeat(count ?? 1));
  }

  /**
   * Press Ctrl+C key combination a specific amount of times.
   *
   * @param count Count of key presses. Default is `1`.
   */
  keyCtrlC(count?: number | undefined): void {
    this.#pty.write(ansi.keyCtrlC.repeat(count ?? 1));
  }

  /**
   * Press Ctrl+D key combination a specific amount of times.
   *
   * @param count Count of key presses. Default is `1`.
   */
  keyCtrlD(count?: number | undefined): void {
    this.#pty.write(ansi.keyCtrlD.repeat(count ?? 1));
  }

  /**
   * Get an array representation of the entire active terminal buffer
   *
   * @returns an array representation of the buffer
   */
  getBuffer(): string[][] {
    return this._getBuffer(0, this.#term.buffer.active.length);
  }

  /**
   * Get an array representation of the visible active terminal buffer
   *
   * @returns an array representation of the buffer
   */
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

  /**
   * Get the terminal's cursor positions
   *
   * @returns the cursor's positions
   */
  getCursor(): CursorPosition {
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

  /**
   * Serialize the terminal into an encoding for snapshots
   *
   * @returns snapshot information
   */
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

  /**
   * Kill the terminal and underlying processes
   */
  kill() {
    process.kill(this.#pty.pid, 9);
  }
}
