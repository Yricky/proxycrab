import { t } from "../i18n";

/** Match the workspace asset store's immutable relative-ID contract. */
export function validateAssetPath(path: string): string | null {
  if (
    !path ||
    path.length > 255 ||
    !/^[a-z0-9_./-]+$/.test(path) ||
    path
      .split("/")
      .some((part) => !part || part.length > 100 || [".", "..", ".metadata"].includes(part))
  )
    return t(
      "路径最多 255 字符，每段最多 100 字符；仅支持小写字母、数字、_、-、. 和目录分隔符 /，不允许空段、.、.. 或 .metadata",
    );
  return null;
}
