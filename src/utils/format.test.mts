import { expect, test } from "vitest";
import { formatActivityCount, formatDateTimeWithZone } from "./format.ts";

test("formatActivityCount folds long connection counts", () => {
  expect(formatActivityCount(0)).toBe("0");
  expect(formatActivityCount(99)).toBe("99");
  expect(formatActivityCount(100)).toBe("99+");
});

test("formatDateTimeWithZone includes the local GMT offset", () => {
  const local = new Date(2026, 7, 31, 14, 30, 25);
  const offsetMinutes = -local.getTimezoneOffset();
  const sign = offsetMinutes >= 0 ? "+" : "-";
  const absoluteOffset = Math.abs(offsetMinutes);
  const hours = String(Math.floor(absoluteOffset / 60)).padStart(2, "0");
  const minutes = String(absoluteOffset % 60).padStart(2, "0");

  expect(
    formatDateTimeWithZone(local.getTime()),
  ).toBe(`2026-08-31 14:30:25 GMT${sign}${hours}:${minutes}`);
});
