// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { IBufferCell, Terminal as XTerminal } from "xterm-headless";
import ms from "pretty-ms";
import { poll } from "../utils/poll.js";
import { Terminal } from "./term.js";

export type Cell = {
  termCell: IBufferCell | undefined;
  x: number;
  y: number;
};

export class Locator {
  private _cells: Cell[] | undefined;

  constructor(
    private readonly _text: string | RegExp,
    private readonly _term: Terminal,
    private readonly _xterm: XTerminal,
    private readonly _full: boolean | undefined = undefined
  ) {}

  private static _getIndicesOf(substring: string, string: string) {
    const indices = [];
    let index = 0;

    while ((index = string.indexOf(substring, index)) > -1) {
      indices.push(index);
      index += substring.length;
    }

    return indices;
  }

  public async resolve(timeout: number): Promise<Cell[]> {
    if (this._cells != null) return this._cells;
    const result = await poll(
      () => {
        const buffer = this._full
          ? this._term.getBuffer()
          : this._term.getViewableBuffer();
        const block = buffer.map((bufferLine) => bufferLine.join("")).join("");

        let index = 0;
        let length = 0;
        if (typeof this._text === "string") {
          const indices = Locator._getIndicesOf(this._text, block);
          if (indices.length > 1) {
            throw new Error(
              `strict mode violation: getByText(${this._text.toString()}) resolved to ${indices.length} elements`
            );
          }
          if (indices.length == 0) {
            return false;
          }
          index = indices[0];
          length = this._text.length;
        } else {
          const matches = Array.from(block.matchAll(this._text));
          if (matches.length > 1) {
            throw new Error(
              `strict mode violation: getByText(${this._text.toString()}) resolved to ${matches.length} elements`
            );
          }
          if (matches.length == 0) {
            return false;
          }
          index = matches[0].index!;
          length = matches[0].length;
        }

        const baseY = this._full ? 0 : this._xterm.buffer.active.baseY;
        this._cells = [];
        for (let y = 0; y < buffer.length; y++) {
          for (let x = 0; x < buffer[y].length ?? 0; x++) {
            if (x + y >= index && x + y < index + length) {
              this._cells.push({
                termCell: this._xterm.buffer.active
                  .getLine(baseY + y)
                  ?.getCell(x),
                x,
                y,
              });
            }
          }
        }
        return true;
      },
      50,
      timeout
    );

    if (!result) {
      throw new Error(
        `locator timeout: getByText(${this._text.toString()}) resolved to 0 elements after ${ms(timeout)}`
      );
    }
    return this._cells!;
  }
}
