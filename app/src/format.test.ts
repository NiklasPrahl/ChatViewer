import { describe, expect, it } from "vitest";

import { byteToUtf16, formatSize, segments, toDate } from "./format";

describe("byteToUtf16", () => {
  it("maps UTF-8 byte offsets to JavaScript indices", () => {
    const map = byteToUtf16("ä😀b");
    expect(map(0)).toBe(0); // ä starts at byte 0
    expect(map(2)).toBe(1); // 😀 starts at byte 2, index 1
    expect(map(6)).toBe(3); // b after the surrogate pair
    expect(map(7)).toBe(4);
  });
});

describe("segments", () => {
  it("applies ranges given in bytes, also after emoji", () => {
    // "😀 bold": the emoji is 4 bytes, so "bold" starts at byte 5.
    const parts = segments({ text: "😀 bold", ranges: [{ start: 5, len: 4, style: { kind: "bold" } }] });
    const bold = parts.find((p) => p.styles.length > 0)!;
    expect(bold.text).toBe("bold");
  });

  it("marks URLs without making them links", () => {
    const parts = segments({ text: "see https://example.org/x now" });
    expect(parts.find((p) => p.url)?.text).toBe("https://example.org/x");
  });
});

describe("toDate", () => {
  it("keeps naive local times as written", () => {
    const d = toDate({ kind: "naive_local", value: "2024-03-01T14:05:09" });
    expect([d.getHours(), d.getMinutes(), d.getSeconds()]).toEqual([14, 5, 9]);
  });
});

describe("formatSize", () => {
  it("uses binary units", () => {
    expect(formatSize(512)).toBe("512 B");
    expect(formatSize(1536)).toBe("1.5 KB");
  });
});
