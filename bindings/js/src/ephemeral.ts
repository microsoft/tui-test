let sessionCounter = 0;

export function uniqueSession(prefix?: string): string {
  const sanitized = (prefix ?? "shell-use").replace(/[^A-Za-z0-9_-]/g, "-");
  const random = (Math.random().toString(36).slice(2) + "0").slice(0, 8);
  const suffix = `-${process.pid}-${random}-${sessionCounter++}`;
  const room = Math.max(1, 64 - suffix.length);
  return `${sanitized.slice(0, room)}${suffix}`;
}
