export function clashServerAddresses(addresses: string[]): string[] {
  return [...new Set(addresses)].filter((address) => {
    const parts = address.split(".");
    if (parts.length !== 4 || parts.some((part) => !/^(0|[1-9]\d{0,2})$/.test(part) || Number(part) > 255)) return false;
    const first = Number(parts[0]);
    return first !== 0 && first !== 127 && !(first >= 224 && first <= 239) && address !== "255.255.255.255";
  });
}

export function clashSubscriptionUrl(server: string, port: number, token: string): string {
  const url = new URL(`http://${server}:${port}/clash.yaml`);
  url.searchParams.set("token", token);
  return url.toString();
}

export function clashImportUrl(subscriptionUrl: string): string {
  return `clash://install-config?url=${encodeURIComponent(subscriptionUrl)}`;
}
