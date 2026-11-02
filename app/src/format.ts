import type { RichText, TextStyle, Timestamp } from "./types";

/** A Date for display. Naive local times are shown as written. */
export function toDate(t: Timestamp): Date {
  if (t.kind === "utc_millis") return new Date(t.value);
  // "2024-01-02T03:04:05" without zone: parse as local time.
  const [d, time = "00:00:00"] = t.value.split("T");
  const [y, m, day] = d.split("-").map(Number);
  const [h, min, s] = time.split(":").map((x) => Number(x.split(".")[0]));
  return new Date(y, m - 1, day, h, min, s || 0);
}

export function formatTime(t: Timestamp): string {
  return toDate(t).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

export function dayKey(t: Timestamp): string {
  const d = toDate(t);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

export function formatDay(t: Timestamp): string {
  return toDate(t).toLocaleDateString(undefined, {
    weekday: "long", year: "numeric", month: "long", day: "numeric",
  });
}

export function formatShortDate(t: Timestamp | undefined): string {
  return t ? toDate(t).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" }) : "";
}

export function formatSize(bytes: number | undefined): string {
  if (bytes === undefined) return "";
  const units = ["B", "KB", "MB", "GB"];
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

export function formatDuration(ms: number | undefined): string {
  if (ms === undefined) return "";
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** A stable, readable colour per participant. */
export function nameColor(id: string): string {
  let h = 0;
  for (const c of id) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${h % 360} 55% var(--name-lightness))`;
}

export interface Segment {
  text: string;
  styles: TextStyle[];
  url?: boolean;
}

/** Maps UTF-8 byte offsets (model) to UTF-16 indices (JavaScript). */
export function byteToUtf16(text: string): (byte: number) => number {
  const offsets: number[] = [];
  let byte = 0;
  for (let i = 0; i < text.length; ) {
    const cp = text.codePointAt(i)!;
    const units = cp > 0xffff ? 2 : 1;
    const bytes = cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;
    for (let b = 0; b < bytes; b++) offsets[byte + b] = i;
    byte += bytes;
    i += units;
  }
  offsets[byte] = text.length;
  return (b) => offsets[Math.min(b, byte)] ?? text.length;
}

const URL_RE = /https?:\/\/[^\s<>"]+/g;

/** Splits rich text into styled segments; URLs are marked (not clickable). */
export function segments(rich: RichText): Segment[] {
  const { text } = rich;
  const toIndex = byteToUtf16(text);
  const ranges = (rich.ranges ?? []).map((r) => ({
    start: toIndex(r.start),
    end: toIndex(r.start + r.len),
    style: r.style,
  }));
  const cuts = new Set<number>([0, text.length]);
  for (const r of ranges) {
    cuts.add(r.start);
    cuts.add(r.end);
  }
  const urls: [number, number][] = [];
  for (const m of text.matchAll(URL_RE)) {
    urls.push([m.index!, m.index! + m[0].length]);
    cuts.add(m.index!);
    cuts.add(m.index! + m[0].length);
  }
  const points = [...cuts].filter((c) => c >= 0 && c <= text.length).sort((a, b) => a - b);
  const out: Segment[] = [];
  for (let i = 0; i < points.length - 1; i++) {
    const [a, b] = [points[i], points[i + 1]];
    if (a === b) continue;
    out.push({
      text: text.slice(a, b),
      styles: ranges.filter((r) => r.start <= a && r.end >= b).map((r) => r.style),
      url: urls.some(([s, e]) => s <= a && e >= b),
    });
  }
  return out;
}
