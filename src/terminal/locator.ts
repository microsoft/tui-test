// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

import { IBufferCell, Terminal as XTerminal } from "xterm-headless";
import ms from "pretty-ms";
import { poll } from "../utils/poll.js";
import { Terminal } from "./term.js";
import { strictModeErrorPrefix } from "../utils/constants.js";

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
    private readonly _full: boolean | undefined = undefined,
    private readonly _strict: boolean = false
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

  /**
   * Gets a locator's search term. This is not supported for direct usage in
   * testing and can break in future releases.
   *
   */
  public searchTerm(): string | RegExp {
    return this._text;
  }

  /**
   * Resolves a locator to a specific set of cells. This is not supported for
   * direct usage in testing and can break in future releases.
   *
   * @param timeout
   * @param isNot
   */
  public async resolve(
    timeout: number,
    isNot: boolean = false
  ): Promise<Cell[] | undefined> {
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
          if (indices.length > 1 && this._strict) {
            throw new Error(
              `${strictModeErrorPrefix}: getByText(${this._text.toString()}) resolved to ${indices.length} elements`
            );
          }
          if (indices.length == 0) {
            return false;
          }
          index = indices[0];
          length = this._text.length;
        } else {
          const matches = Array.from(block.matchAll(this._text));
          if (matches.length > 1 && this._strict) {
            throw new Error(
              `${strictModeErrorPrefix}: getByText(${this._text.toString()}) resolved to ${matches.length} elements`
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
            const pos = x + y * buffer[y].length;
            if (pos >= index && pos < index + length) {
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
      timeout,
      isNot
    );

    if (!result && !isNot) {
      throw new Error(
        `locator timeout: getByText(${this._text.toString()}) resolved to 0 elements after ${ms(timeout)}`
      );
    }
    if (!result && isNot) {
      this._cells = undefined;
    }
    return this._cells;
  }
}
