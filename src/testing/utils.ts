export async function poll(callback: () => boolean | Promise<boolean>, delay: number, timeout: number): Promise<boolean> {
  return await _poll(callback, Date.now(), delay, timeout);
}

async function _poll(callback: () => boolean | Promise<boolean>, startTime: number, delay: number, timeout: number): Promise<boolean> {
  const result = await Promise.resolve(callback());
  if (result) {
    return true;
  }
  if (startTime + timeout < Date.now()) {
    return false;
  }
  return new Promise((resolve) => setTimeout(() => resolve(_poll(callback, startTime, delay, timeout)), delay));
}
