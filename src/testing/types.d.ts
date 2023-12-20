import { Matchers, AsymmetricMatchers, BaseExpect } from "expect";
import { Terminal } from "../core/term.ts";

interface TerminalAssertions {
  /**
   * Checks that Terminal has the provided text or RegExp.
   *
   * **Usage**
   *
   * ```js
   * await expect(terminal).toHaveValue("> ");
   * ```
   *
   * @param options
   */
  toHaveValue(
    value: string | RegExp,
    options?: {
      /**
       * Time to retry the assertion for in milliseconds. Defaults to `timeout` in `TestConfig.expect`.
       */
      timeout?: number;
      /**
       * Whether to check the entire terminal buffer for the value instead of only the visible section.
       */
      full?: number;
    }
  ): Promise<void>;
}

declare type BaseMatchers<T> = Matchers<void, T> & Inverse<Matchers<void, T>> & PromiseMatchers<T>;
declare type AllowedGenericMatchers<T> = Pick<Matchers<void, T>, "toBe" | "toBeDefined" | "toBeFalsy" | "toBeNull" | "toBeTruthy" | "toBeUndefined">;
declare type SpecificMatchers<T> = T extends Terminal ? TerminalAssertions & AllowedGenericMatchers<T> : BaseMatchers<T>;

export declare type Expect = {
  <T = unknown>(actual: T): SpecificMatchers<T>;
} & BaseExpect &
  AsymmetricMatchers &
  Inverse<Omit<AsymmetricMatchers, "any" | "anything">>;

declare type PromiseMatchers<T = unknown> = {
  /**
   * Unwraps the reason of a rejected promise so any other matcher can be chained.
   * If the promise is fulfilled the assertion fails.
   */
  rejects: Matchers<Promise<void>, T> & Inverse<Matchers<Promise<void>, T>>;
  /**
   * Unwraps the value of a fulfilled promise so any other matcher can be chained.
   * If the promise is rejected the assertion fails.
   */
  resolves: Matchers<Promise<void>, T> & Inverse<Matchers<Promise<void>, T>>;
};

declare type Inverse<Matchers> = {
  /**
   * Inverse next matcher. If you know how to test something, `.not` lets you test its opposite.
   */
  not: Matchers;
};
