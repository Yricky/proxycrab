import type { Backend } from "./backend";
import { getBackend } from "./runtime-backend";

/** Access the current Backend implementation from any component. */
export function useBackend(): Backend {
  return getBackend();
}
