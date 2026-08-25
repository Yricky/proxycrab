export const DEFAULT_SHARE_HOURS = 24;
export const MAX_SHARE_HOURS = 720;

export function parseShareHours(value: string): number | null {
  if (!/^\d+$/.test(value)) return null;
  const hours = Number(value);
  return Number.isInteger(hours) && hours >= 1 && hours <= MAX_SHARE_HOURS ? hours : null;
}

export function buildSessionShareLinks(
  addresses: string[],
  port: number,
  token: string,
): string[] {
  return [...new Set(addresses)]
    .filter((address) => address !== "127.0.0.1" && address !== "0.0.0.0")
    .map(
      (address) =>
        `http://${address}:${port}/session?token=${encodeURIComponent(token)}`,
    );
}
