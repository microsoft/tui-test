// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

export async function poll(
  callback: () => boolean | Promise<boolean>,
  delay: number,
  timeout: number,
  isNot?: boolean
): Promise<boolean> {
  return await _poll(callback, Date.now(), delay, timeout, isNot ?? false);
}

async function _poll(
  callback: () => boolean | Promise<boolean>,
  startTime: number,
  delay: number,
  timeout: number,
  isNot: boolean
): Promise<boolean> {
  const result = await Promise.resolve(callback());
  if (!isNot && result) {
    return true;
  }
  if (isNot && !result) {
    return false;
  }
  if (startTime + timeout < Date.now()) {
    return isNot;
  }
  return new Promise((resolve) =>
    setTimeout(
      () => resolve(_poll(callback, startTime, delay, timeout, isNot)),
      delay
    )
  );
}
