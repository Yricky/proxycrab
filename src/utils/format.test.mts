import assert from "node:assert/strict";
import test from "node:test";
import { formatDateTimeWithZone } from "./format.ts";

test("formatDateTimeWithZone includes the local GMT offset", () => {
  const local = new Date(2026, 7, 31, 14, 30, 25);
  const offsetMinutes = -local.getTimezoneOffset();
  const sign = offsetMinutes >= 0 ? "+" : "-";
  const absoluteOffset = Math.abs(offsetMinutes);
  const hours = String(Math.floor(absoluteOffset / 60)).padStart(2, "0");
  const minutes = String(absoluteOffset % 60).padStart(2, "0");

  assert.equal(
    formatDateTimeWithZone(local.getTime()),
    `2026-08-31 14:30:25 GMT${sign}${hours}:${minutes}`,
  );
});
