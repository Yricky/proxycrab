import type { HttpApiChange } from "../api/generated/HttpApiChange";
import type { HttpApiResource } from "../api/generated/HttpApiResource";

export type ResourceKind = HttpApiResource["kind"];

export function resourceKinds(change: HttpApiChange): ResourceKind[] {
  return change.resources.map((resource) => resource.kind);
}
