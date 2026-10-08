import { describe, expect, it } from "vitest";
import { formatElapsed, formatStatus } from "./elapsed";

describe("formatElapsed", () => {
  it("formats under a minute as MM:SS", () => {
    expect(formatElapsed(0, 43_000)).toBe("00:43");
  });

  it("formats minutes and seconds", () => {
    expect(formatElapsed(0, 8 * 60_000 + 17_000)).toBe("08:17");
  });

  it("keeps two-digit minutes under an hour", () => {
    expect(formatElapsed(0, 59 * 60_000 + 59_000)).toBe("59:59");
  });

  it("switches to H:MM:SS at one hour", () => {
    expect(formatElapsed(0, 3_600_000)).toBe("1:00:00");
    expect(formatElapsed(0, 3_600_000 + 4 * 60_000 + 32_000)).toBe("1:04:32");
  });

  it("does not pad the hour", () => {
    expect(formatElapsed(0, 12 * 3_600_000 + 5_000)).toBe("12:00:05");
  });

  it("truncates rather than rounds, so the clock never reads ahead", () => {
    expect(formatElapsed(0, 1_999)).toBe("00:01");
    expect(formatElapsed(0, 999)).toBe("00:00");
  });

  it("clamps a future start time instead of showing a negative duration", () => {
    // The producer's clock can be ahead of ours; that is not a negative age.
    expect(formatElapsed(10_000, 4_000)).toBe("00:00");
  });

  it("measures from the real difference, not from the absolute timestamp", () => {
    const startedAt = 1_790_340_000_000;
    expect(formatElapsed(startedAt, startedAt + 65_000)).toBe("01:05");
  });
});

describe("formatStatus", () => {
  it("labels the two statuses the note normally shows", () => {
    expect(formatStatus("running")).toBe("运行中");
    expect(formatStatus("waiting")).toBe("等待中");
  });

  it("labels the remaining protocol statuses rather than leaking the raw value", () => {
    expect(formatStatus("completed")).toBe("已完成");
    expect(formatStatus("cancelled")).toBe("已取消");
    expect(formatStatus("failed")).toBe("失败");
    expect(formatStatus("unknown")).toBe("未知");
  });

  it("未知状态会回退为“未知”", () => {
    expect(formatStatus("something-new")).toBe("未知");
  });
});
