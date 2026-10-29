import { afterEach, expect, test, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { createTauriBackend } from "./tauri-backend.ts";
import { BodyFetchError, DEFAULT_BODY_MAX_SIZE, UI_BODY_MAX_SIZE } from "./body.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
afterEach(() => {
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

test.each([
  { kind: "log", id: 42, sessionId: 3 },
  { kind: "interceptor", id: 42, sessionId: 3, executionId: 17 },
  { kind: "breakpoint", id: 7 },
] as const)("desktop preview loads $kind bytes through IPC without HTTP", async (target) => {
  const fetchMock = vi.fn();
  vi.stubGlobal("fetch", fetchMock);
  vi.mocked(invoke).mockResolvedValue({ bytes: [0, 255, 13, 10], contentType: "image/png" });
  const body = await createTauriBackend().fetchBody(target, "response");
  expect(body).toEqual({
    bytes: new Uint8Array([0, 255, 13, 10]),
    decodedSize: 4,
    contentType: "image/png",
  });
  expect(invoke).toHaveBeenCalledExactlyOnceWith("fetch_body", {
    target,
    side: "response",
    maxSize: DEFAULT_BODY_MAX_SIZE,
  });
  expect(fetchMock).not.toHaveBeenCalled();
});

test("desktop preview preserves size errors for manual loading and forwards the larger limit", async () => {
  const target = { kind: "log", id: 42, sessionId: 3 } as const;
  const error = {
    code: "body_too_large",
    message: "too large",
    actual_size: 32000000,
    max_size: DEFAULT_BODY_MAX_SIZE,
  };
  vi.mocked(invoke).mockRejectedValueOnce(error);
  await expect(createTauriBackend().fetchBody(target, "request")).rejects.toMatchObject({
    name: "BodyFetchError",
    code: error.code,
    actualSize: error.actual_size,
    maxSize: error.max_size,
  });
  vi.mocked(invoke).mockResolvedValueOnce({ bytes: [], contentType: "application/octet-stream" });
  await createTauriBackend().fetchBody(target, "request", UI_BODY_MAX_SIZE);
  expect(invoke).toHaveBeenLastCalledWith("fetch_body", {
    target,
    side: "request",
    maxSize: UI_BODY_MAX_SIZE,
  });
});

test.each([0, -1, 1.5, NaN, UI_BODY_MAX_SIZE + 1])(
  "invalid preview limit %s never invokes IPC",
  async (maxSize) => {
    await expect(
      createTauriBackend().fetchBody({ kind: "breakpoint", id: 7 }, "response", maxSize),
    ).rejects.toBeInstanceOf(BodyFetchError);
    expect(invoke).not.toHaveBeenCalled();
  },
);

test("desktop Session sharing groups redaction and persistence into config", async () => {
  const state = { session_id: 7, enabled: true, persistent: true, token: "same-token" };
  vi.mocked(invoke).mockResolvedValue(state);
  const backend = createTauriBackend();
  expect(
    await backend.enableSessionShare(7, { redactionScript: "mask", persistent: true }),
  ).toEqual(state);
  expect(invoke).toHaveBeenLastCalledWith(
    "enable_session_share",
    {
      sessionId: 7,
      sessionShareConfig: { redaction_script: "mask", persistent: true },
    },
    undefined,
  );
  expect(await backend.enableSessionShare(7, { redactionScript: null, persistent: false })).toEqual(
    state,
  );
  expect(invoke).toHaveBeenLastCalledWith(
    "enable_session_share",
    {
      sessionId: 7,
      sessionShareConfig: { redaction_script: null, persistent: false },
    },
    undefined,
  );
  await backend.enableSessionShare(7);
  expect(invoke).toHaveBeenLastCalledWith(
    "enable_session_share",
    {
      sessionId: 7,
      sessionShareConfig: undefined,
    },
    undefined,
  );
});

test("desktop batch share summaries use their dedicated read IPC", async () => {
  const payload = { shares: [{ session_id: 3, persistent: true }] };
  vi.mocked(invoke).mockResolvedValue(payload);
  expect(await createTauriBackend().listSessionShares!()).toEqual(payload);
  expect(invoke).toHaveBeenLastCalledWith("list_session_shares", undefined, undefined);
});

test("desktop writer opens before data, sends bounded raw chunks and commits", async () => {
  vi.mocked(invoke).mockResolvedValueOnce("writer-7").mockResolvedValue(undefined);
  const writer = await createTauriBackend().saveFile({ filename: "capture.har" });
  expect(invoke).toHaveBeenCalledExactlyOnceWith(
    "save_file_open",
    { filename: "capture.har" },
    undefined,
  );
  await writer!.append(new Uint8Array(256 * 1024 + 1));
  const calls = vi.mocked(invoke).mock.calls.filter(([method]) => method === "save_file_append");
  expect(calls.map(([, data]) => (data as Uint8Array).length)).toEqual([256 * 1024, 1]);
  expect(calls[0]![2]).toEqual({ headers: { "x-proxycrab-writer": "writer-7" } });
  await writer!.markAsFinish();
  expect(invoke).toHaveBeenLastCalledWith("save_file_finish", { handle: "writer-7" }, undefined);
});
test("desktop picker cancellation returns no writer", async () => {
  vi.mocked(invoke).mockResolvedValueOnce(null);
  expect(await createTauriBackend().saveFile({ filename: "capture.har" })).toBeNull();
  expect(invoke).toHaveBeenCalledTimes(1);
});
