/**
 * JWT 解码与验签工具，功能对齐 jwt.io 的 JWT Decoder：
 *  - 解码 header / payload（base64url → UTF-8 JSON）
 *  - 支持 HS256/384/512、RS256/384/512、PS256/384/512、ES256/384/512、none
 *  - 密钥支持：HMAC 明文密钥、PEM 公钥（SPKI / PKCS#1 / SEC1）、JWK JSON
 *  - EdDSA (Ed25519)：WebKit 的 Web Crypto 不支持，返回 unsupported
 *
 * 全部逻辑为纯前端实现，不依赖后端。
 */

export const JWT_ALGORITHMS = [
  "HS256",
  "HS384",
  "HS512",
  "RS256",
  "RS384",
  "RS512",
  "PS256",
  "PS384",
  "PS512",
  "ES256",
  "ES384",
  "ES512",
  "EdDSA",
] as const;

export type JwtAlgorithm = (typeof JWT_ALGORITHMS)[number];

/** 与 jwt.io 一致：下拉框里也包含 none（不验签）。 */
export const JWT_ALGORITHM_OPTIONS: readonly string[] = [...JWT_ALGORITHMS, "none"];

const HASH_BY_ALG: Record<string, string> = {
  HS256: "SHA-256",
  HS384: "SHA-384",
  HS512: "SHA-512",
  RS256: "SHA-256",
  RS384: "SHA-384",
  RS512: "SHA-512",
  PS256: "SHA-256",
  PS384: "SHA-384",
  PS512: "SHA-512",
  ES256: "SHA-256",
  ES384: "SHA-384",
  ES512: "SHA-512",
};

const SALT_LENGTH: Record<string, number> = {
  PS256: 32,
  PS384: 48,
  PS512: 64,
};

const CURVE_BY_ALG: Record<string, string> = {
  ES256: "P-256",
  ES384: "P-384",
  ES512: "P-521",
};

export interface DecodedJwt {
  ok: boolean;
  /** 解析失败时的错误信息；token 为空时为 ""。 */
  error: string;
  headerRaw: string;
  payloadRaw: string;
  signatureRaw: string;
  headerText: string;
  payloadText: string;
  headerJson: unknown;
  payloadJson: unknown;
  alg: string;
  /** 无签名段或 alg=none 的令牌（JWS 未签名场景）。 */
  unsecured: boolean;
}

const EMPTY: DecodedJwt = {
  ok: false,
  error: "",
  headerRaw: "",
  payloadRaw: "",
  signatureRaw: "",
  headerText: "",
  payloadText: "",
  headerJson: null,
  payloadJson: null,
  alg: "",
  unsecured: false,
};

// ---------- base64 / base64url ----------

export function base64UrlDecode(text: string): Uint8Array {
  const b64 = text.replace(/-/g, "+").replace(/_/g, "/");
  const padded = b64 + "=".repeat((4 - (b64.length % 4)) % 4);
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

export function base64UrlEncode(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function base64Decode(text: string): Uint8Array {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

const textDecoder = new TextDecoder("utf-8");
const textEncoder = new TextEncoder();

function tryJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

// ---------- 解码 ----------

export function decodeToken(token: string): DecodedJwt {
  const trimmed = token.trim();
  if (!trimmed) return EMPTY;
  const parts = trimmed.split(".");
  if (parts.length < 2 || parts.length > 3) {
    return {
      ...EMPTY,
      ok: false,
      error: "不是合法的 JWT：应为 header.payload[.signature] 三段结构",
    };
  }
  const [headerRaw, payloadRaw, signatureRaw = ""] = parts;
  let headerText: string;
  let payloadText: string;
  try {
    headerText = textDecoder.decode(base64UrlDecode(headerRaw));
    payloadText = textDecoder.decode(base64UrlDecode(payloadRaw));
  } catch {
    return {
      ...EMPTY,
      ok: false,
      headerRaw,
      payloadRaw,
      signatureRaw,
      error: "Base64url 解码失败：令牌中包含非法字符",
    };
  }
  const headerJson = tryJson(headerText);
  const payloadJson = tryJson(payloadText);
  const alg = (headerJson && typeof headerJson === "object" ? (headerJson as { alg?: unknown }).alg : undefined) ?? "";
  const unsecured = parts.length === 2 || String(alg) === "none";
  return {
    ok: true,
    error: "",
    headerRaw,
    payloadRaw,
    signatureRaw,
    headerText,
    payloadText,
    headerJson,
    payloadJson,
    alg: String(alg),
    unsecured,
  };
}

// ---------- 验签 ----------

export type VerifyStatus =
  | "verified"
  | "invalid"
  | "waiting"
  | "key-error"
  | "unsupported"
  | "unsecured"
  | "token-error";

export interface VerifyOutcome {
  status: VerifyStatus;
  message: string;
}

// ---------- 极简 DER (ASN.1) 读取 ----------

function readTlv(bytes: Uint8Array, offset = 0): { tag: number; value: Uint8Array; next: number } {
  if (offset + 2 > bytes.length) throw new Error("DER 数据截断");
  const tag = bytes[offset];
  let len = bytes[offset + 1];
  let pos = offset + 2;
  if (len & 0x80) {
    const count = len & 0x7f;
    if (count === 0 || pos + count > bytes.length) throw new Error("DER 长度字段无效");
    len = 0;
    for (let i = 0; i < count; i++) len = len * 256 + bytes[pos + i];
    pos += count;
  }
  if (pos + len > bytes.length) throw new Error("DER 内容截断");
  return { tag, value: bytes.slice(pos, pos + len), next: pos + len };
}

/** 复制为确定 ArrayBuffer 承载的视图（满足 Web Crypto 的 BufferSource 类型约束）。 */
function toArrayBuffer(bytes: Uint8Array): Uint8Array<ArrayBuffer> {
  return new Uint8Array(bytes);
}

function pkcs1ToJwk(der: Uint8Array): JsonWebKey {
  const outer = readTlv(der);
  if (outer.tag !== 0x30) throw new Error("RSA 公钥不是合法的 DER SEQUENCE");
  const nTlv = readTlv(outer.value);
  if (nTlv.tag !== 0x02) throw new Error("RSA 公钥缺少模数 n");
  const eTlv = readTlv(outer.value, nTlv.next);
  if (eTlv.tag !== 0x02) throw new Error("RSA 公钥缺少指数 e");
  let n = nTlv.value;
  let e = eTlv.value;
  let start = 0;
  while (start < n.length - 1 && n[start] === 0) start++;
  n = n.slice(start);
  start = 0;
  while (start < e.length - 1 && e[start] === 0) start++;
  e = e.slice(start);
  return { kty: "RSA", n: base64UrlEncode(n), e: base64UrlEncode(e) };
}

// ---------- 密钥解析 ----------

type PemKind = "spki" | "pkcs1" | "cert";

function detectPem(text: string): PemKind | null {
  if (/BEGIN CERTIFICATE/.test(text)) return "cert";
  if (/BEGIN RSA PUBLIC KEY/.test(text)) return "pkcs1";
  if (/BEGIN (EC )?PUBLIC KEY/.test(text)) return "spki";
  return null;
}

function pemToDer(pem: string): Uint8Array {
  const body = pem
    .replace(/-----BEGIN [A-Z0-9 ]+-----/, "")
    .replace(/-----END [A-Z0-9 ]+-----/, "")
    .replace(/\s+/g, "");
  return base64Decode(body);
}

/** 非对称算法的密钥导入参数（RSA-PSS 的 saltLength 仅在验签时指定）。 */
function asymmetricImportParams(alg: string): RsaHashedImportParams | EcKeyImportParams {
  const hash = HASH_BY_ALG[alg] ?? "SHA-256";
  if (alg.startsWith("RS")) return { name: "RSASSA-PKCS1-v1_5", hash };
  if (alg.startsWith("PS")) return { name: "RSA-PSS", hash };
  if (alg.startsWith("ES")) return { name: "ECDSA", namedCurve: CURVE_BY_ALG[alg] ?? "P-256" };
  throw new Error(`不支持的非对称算法：${alg}`);
}

/** 验签参数：Web Crypto 的 ECDSA 签名即裸 r||s（P1363），直接透传。 */
function verifyParams(alg: string): AlgorithmIdentifier | RsaPssParams | EcdsaParams {
  if (alg.startsWith("HS")) return "HMAC";
  if (alg.startsWith("RS")) return "RSASSA-PKCS1-v1_5";
  if (alg.startsWith("PS")) return { name: "RSA-PSS", saltLength: SALT_LENGTH[alg] ?? 32 };
  if (alg.startsWith("ES")) return { name: "ECDSA", hash: HASH_BY_ALG[alg] ?? "SHA-256" };
  throw new Error(`不支持的非对称算法：${alg}`);
}

/** 拷贝 JWK 并补充 alg 字段（WebKit 对 RSA-PSS 等 JWK 导入较严格）。 */
function normalizeJwk(jwk: JsonWebKey, alg: string): JsonWebKey {
  const copy: JsonWebKey = { ...jwk };
  if (!copy.alg) copy.alg = alg;
  return copy;
}

async function importJwkKey(alg: string, jwk: JsonWebKey): Promise<CryptoKey> {
  const hash = HASH_BY_ALG[alg] ?? "SHA-256";
  if (alg.startsWith("HS")) {
    if (jwk.kty !== "oct" || !jwk.k) throw new Error("HS 算法需要 kty=oct 的 JWK（含 k）");
    return crypto.subtle.importKey("jwk", normalizeJwk(jwk, alg), { name: "HMAC", hash }, false, ["verify"]);
  }
  if (alg.startsWith("RS") || alg.startsWith("PS")) {
    if (jwk.kty !== "RSA" || !jwk.n || !jwk.e) throw new Error("RSA 算法需要 kty=RSA 的 JWK（含 n/e）");
    return crypto.subtle.importKey("jwk", normalizeJwk(jwk, alg), asymmetricImportParams(alg), false, ["verify"]);
  }
  if (alg.startsWith("ES")) {
    if (jwk.kty !== "EC" || !jwk.crv || !jwk.x || !jwk.y) throw new Error("ES 算法需要 kty=EC 的 JWK（含 crv/x/y）");
    return crypto.subtle.importKey("jwk", normalizeJwk(jwk, alg), asymmetricImportParams(alg), false, ["verify"]);
  }
  throw new Error(`不支持通过 JWK 导入的算法：${alg}`);
}

async function importVerificationKey(alg: string, keyText: string): Promise<CryptoKey> {
  const trimmed = keyText.trim();

  if (trimmed.startsWith("{")) {
    // JWK JSON（HS 可用 kty=oct，非对称可用 kty=RSA / kty=EC）
    let jwk: JsonWebKey;
    try {
      jwk = JSON.parse(trimmed) as JsonWebKey;
    } catch {
      throw new Error("JWK JSON 解析失败：不是合法的 JSON");
    }
    return importJwkKey(alg, jwk);
  }

  if (alg.startsWith("HS")) {
    // HMAC：整段文本按 UTF-8 作为密钥
    return crypto.subtle.importKey("raw", toArrayBuffer(textEncoder.encode(trimmed)), { name: "HMAC", hash: HASH_BY_ALG[alg] ?? "SHA-256" }, false, ["verify"]);
  }

  const pemKind = detectPem(trimmed);
  if (pemKind === null) {
    throw new Error("密钥格式无法识别：请粘贴 HMAC 密钥、PEM 公钥（-----BEGIN ... PUBLIC KEY-----）或 JWK JSON");
  }
  if (pemKind === "cert") {
    throw new Error("不支持 X.509 证书，请粘贴 PEM 公钥或 JWK JSON");
  }
  const der = pemToDer(trimmed);
  if (pemKind === "pkcs1") {
    if (!alg.startsWith("RS") && !alg.startsWith("PS")) {
      throw new Error(`RSA PKCS#1 公钥不能用于 ${alg}，请选择 RS/PS 系列算法`);
    }
    return crypto.subtle.importKey("jwk", pkcs1ToJwk(der), asymmetricImportParams(alg), false, ["verify"]);
  }
  // SPKI（BEGIN PUBLIC KEY / BEGIN EC PUBLIC KEY）
  return crypto.subtle.importKey("spki", toArrayBuffer(der), asymmetricImportParams(alg), false, ["verify"]);
}

function keyErrorMessage(error: unknown): string {
  if (error instanceof DOMException) {
    switch (error.name) {
      case "NotSupportedError":
        return "密钥算法与所选算法不匹配（或当前环境不支持）";
      case "DataError":
        return "密钥与所选算法不匹配，或密钥格式不正确";
      case "SyntaxError":
        return "密钥内容不是合法的 DER/PEM 数据";
      default:
        return `密钥导入失败：${error.name}`;
    }
  }
  return error instanceof Error ? error.message : String(error);
}

/**
 * 验签（与 jwt.io 行为对齐）：用所选算法与密钥校验 header.payload 的签名。
 * 密钥格式按算法族自动判断：HS 系列用明文密钥；RS/PS/ES 系列支持 PEM 公钥与 JWK。
 */
export async function verifySignature(token: string, alg: string, keyText: string): Promise<VerifyOutcome> {
  const decoded = decodeToken(token);
  if (!decoded.ok) {
    return { status: "token-error", message: decoded.error || "令牌格式无效" };
  }
  if (decoded.unsecured) {
    return { status: "unsecured", message: "该令牌未签名（alg=none 或无签名段）" };
  }
  if (!alg) return { status: "waiting", message: "未选择算法" };
  if (alg === "EdDSA") {
    return { status: "unsupported", message: "当前 WebView 不支持 EdDSA (Ed25519) 验签" };
  }
  if (!(alg in HASH_BY_ALG)) {
    return { status: "unsupported", message: `不支持的算法：${alg}` };
  }
  if (!keyText.trim()) return { status: "waiting", message: "输入密钥后开始验证" };

  const signingInput = textEncoder.encode(`${decoded.headerRaw}.${decoded.payloadRaw}`);
  let signature: Uint8Array;
  try {
    signature = base64UrlDecode(decoded.signatureRaw);
  } catch {
    return { status: "invalid", message: "签名字段不是合法的 Base64url" };
  }
  if (signature.length === 0) return { status: "invalid", message: "签名数据为空" };

  try {
    const key = await importVerificationKey(alg, keyText);
    // Web Crypto 的 ECDSA 签名即为裸 r||s（P1363），无需转换
    const verified = await crypto.subtle.verify(verifyParams(alg), key, toArrayBuffer(signature), toArrayBuffer(signingInput));
    return verified
      ? { status: "verified", message: "签名有效（Signature Verified）" }
      : { status: "invalid", message: "签名无效（Signature Invalid）" };
  } catch (error) {
    return { status: "key-error", message: keyErrorMessage(error) };
  }
}
