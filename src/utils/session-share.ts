function isLoopback(address: string): boolean {
  return address === "127.0.0.1";
}

export function buildSessionShareLinks(
  addresses: string[],
  port: number,
  token: string,
): string[] {
  const usable = [...new Set(addresses)].filter((address) => address !== "0.0.0.0");
  const external = usable.filter((address) => !isLoopback(address));
  const selected = external.length > 0 ? external : usable.filter(isLoopback);
  const encodedToken = encodeURIComponent(token);

  return selected.map(
    (address) => `http://${address}:${port}/session?token=${encodedToken}`,
  );
}
