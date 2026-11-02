// Synthetic backend for developing the UI in a browser (`npm run dev:mock`).
// Fictional people and random words only; never real data.

import type { Api } from "./api";
import type {
  Attachment, ChatInfo, ChatSummary, Content, Message, Participant, SourceReport, Theme,
} from "./types";

const WORDS =
  "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua"
    .split(" ");

function rng(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 2 ** 32;
  };
}

function sentence(r: () => number, n = 3 + Math.floor(r() * 14)): string {
  const w = Array.from({ length: n }, () => WORDS[Math.floor(r() * WORDS.length)]);
  w[0] = w[0][0].toUpperCase() + w[0].slice(1);
  return w.join(" ") + ".";
}

const svg = (w: number, h: number, hue: number, label: string): string =>
  "data:image/svg+xml;utf8," +
  encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}">` +
      `<rect width="100%" height="100%" fill="hsl(${hue},55%,60%)"/>` +
      `<text x="50%" y="50%" font-family="sans-serif" font-size="${Math.round(h / 8)}" ` +
      `fill="white" text-anchor="middle" dominant-baseline="middle">${label}</text></svg>`,
  );

const people: Participant[] = [
  { id: "p1", display_name: "Alice Example", is_self: "yes" },
  { id: "p2", display_name: "Bob Sample", is_self: "no" },
  { id: "p3", display_name: "Carol Test", is_self: "no" },
  { id: "p4", display_name: "Dave Demo", is_self: "no" },
];

const START = Date.UTC(2024, 0, 3, 8, 0, 0);

function image(n: number, available = true): Attachment {
  return {
    kind: "image",
    mime: "image/svg+xml",
    file_name: `photo-${n}.jpg`,
    size: 120_000 + n,
    width: 800,
    height: 600,
    availability: available
      ? { status: "available", blob: `img:${n}`, integrity: "unchecked" }
      : { status: "not_in_backup" },
  };
}

/** Message `i` of a chat, generated deterministically. */
function message(chat: string, i: number, members: Participant[], showcase: boolean): Message {
  const r = rng(i * 7919 + chat.length);
  const author = members[Math.floor(r() * members.length)];
  const ts = { kind: "utc_millis" as const, value: START + i * 47 * 60_000 };
  let content: Content = { type: "text", text: sentence(r) };
  const extra: Partial<Message> = {};
  const roll = showcase ? i % 24 : Math.floor(r() * 40);
  switch (roll) {
    case 1:
      content = {
        type: "text",
        // Signal stores mentions as U+FFFC with a range (3 bytes in UTF-8).
        text: "Bold, italic and a mention of \uFFFC in one line.\nWith a second line.",
        ranges: [
          { start: 0, len: 4, style: { kind: "bold" } },
          { start: 6, len: 6, style: { kind: "italic" } },
          { start: 30, len: 3, style: { kind: "mention", value: "p2" } },
        ],
      };
      break;
    case 3:
      content = { type: "media", attachments: [image(i)], caption: { text: sentence(r, 5) } };
      break;
    case 5:
      content = { type: "media", attachments: [image(i, false)] };
      break;
    case 7:
      content = {
        type: "sticker",
        emoji: "😀",
        image: {
          kind: "sticker",
          mime: "image/svg+xml",
          availability: i % 2 ? { status: "available", blob: `stk:${i}`, integrity: "unchecked" } : { status: "not_in_backup" },
        },
      };
      break;
    case 9:
      content = {
        type: "media",
        attachments: [{
          kind: "voice", mime: "audio/aac", duration_ms: 14_000,
          waveform: Array.from({ length: 60 }, (_, k) => Math.floor(128 + 120 * Math.sin(k / 3) * r())),
          availability: { status: "not_in_backup" },
        }],
      };
      break;
    case 11:
      content = {
        type: "media",
        attachments: [{
          kind: "document", mime: "application/pdf", file_name: "Synthetic report.pdf", size: 482_133,
          availability: { status: "missing" },
        }],
      };
      break;
    case 13:
      content = { type: "location", latitude: 52.52, longitude: 13.405, name: "Synthetic Square" };
      break;
    case 15:
      content = {
        type: "poll", question: "Lunch today?",
        options: [{ text: "Pizza", votes: 3 }, { text: "Salad", votes: 1 }, { text: "Soup", votes: 0 }],
      };
      break;
    case 17:
      content = { type: "system", kind: "member_added", text: "Alice Example added Dave Demo" };
      break;
    case 19:
      content = { type: "deleted" };
      break;
    case 21:
      content = { type: "unsupported", source_kind: "payment", note: "Payment notification (not shown)" };
      break;
  }
  if (roll === 2 && i > 0) {
    extra.quote = { target: `${chat}-${i - 1}`, author: "p2", snippet: { text: sentence(r, 6) } };
  }
  if (roll % 5 === 4) {
    extra.reactions = [
      { emoji: "👍", author: "p2" },
      { emoji: "❤️", author: "p3" },
      { emoji: "👍", author: "p1" },
    ];
  }
  if (roll === 6) extra.flags = { edited: true };
  const system = content.type === "system";
  return {
    id: `${chat}-${i}`,
    seq: i,
    timestamp: ts,
    author: system ? { type: "system" } : { type: "participant", id: author.id },
    content,
    ...extra,
  };
}

interface MockChat {
  summary: ChatSummary;
  members: Participant[];
  showcase: boolean;
}

const chats: MockChat[] = [
  {
    summary: { id: "c1", title: "Synthetic Team", kind: "group", participants: 4, messages: 480 },
    members: people,
    showcase: true,
  },
  {
    summary: { id: "c2", title: "Bob Sample", kind: "direct", participants: 2, messages: 120 },
    members: people.slice(0, 2),
    showcase: false,
  },
  {
    summary: { id: "c3", title: "Huge chat (100,000 messages)", kind: "direct", participants: 2, messages: 100_000 },
    members: [people[0], people[2]],
    showcase: false,
  },
];
for (const c of chats) {
  c.summary.first = { kind: "utc_millis", value: START };
  c.summary.last = { kind: "utc_millis", value: START + (c.summary.messages - 1) * 47 * 60_000 };
}

const report: SourceReport = {
  app: "signal",
  platform: "desktop",
  variant: "signal-local-backup-v1-metadata-v1",
  source_version: "Desktop 8.33.0 (backup format 1, metadata 1)",
  confidence: 100,
  warnings: ["This is synthetic demo data (browser preview without the desktop backend)."],
  limitations: ["media that were not on the device when the backup was made are not included"],
  counters: { chats: 3, chat_items: 100_600 },
};

const delay = <T,>(value: T, ms = 120): Promise<T> =>
  new Promise((resolve) => setTimeout(() => resolve(value), ms));

export function mockApi(): Api {
  let theme: Theme = "auto";
  const progress = new Set<(p: { done: number; total: number }) => void>();
  return {
    inspect: async (path) =>
      delay({
        importer: path.includes("Signal") ? "signal-local" : "whatsapp",
        secret_prompt: path.includes("Signal") ? "Signal recovery key (64 characters)" : null,
      }),
    openArchive: async (request) => {
      if (request.secret !== undefined && request.secret.replace(/[\s-]/g, "").length !== 64) {
        throw { message: "invalid recovery key: fewer than 64 characters" };
      }
      return delay({ handle: 1, importer: "signal-local", report, chats: chats.map((c) => c.summary) }, 400);
    },
    openChat: async (_handle, chat) => {
      const c = chats.find((x) => x.summary.id === chat)!;
      const info: ChatInfo = {
        conversation: { id: chat, kind: c.summary.kind, title: c.summary.title, participants: c.members.map((m) => m.id) },
        participants: c.members,
        message_count: c.summary.messages,
      };
      return delay(info, 200);
    },
    messages: async (_handle, chat, offset, limit) => {
      const c = chats.find((x) => x.summary.id === chat)!;
      const end = Math.min(offset + limit, c.summary.messages);
      const out: Message[] = [];
      for (let i = offset; i < end; i++) out.push(message(chat, i, c.members, c.showcase));
      return delay(out, 30);
    },
    messageIndex: async (_handle, _chat, message) => {
      const n = Number(message.split("-").pop());
      return Number.isFinite(n) ? n : null;
    },
    exportChat: async (_h, _c, format) =>
      delay({ messages: 480, media_written: format === "cva" ? 40 : 0, media_failed: 0, lossless: format === "json" || format === "cva" }, 600),
    exportChats: async (_h, ids, format, folder) => {
      for (let i = 1; i <= ids.length; i++) {
        progress.forEach((cb) => cb({ done: i, total: ids.length }));
        await delay(null, 150);
      }
      const ext = format === "cva" ? "zip" : format;
      return {
        folder,
        exported: ids.map((id) => {
          const c = chats.find((x) => x.summary.id === id)!;
          return { chat: id, file: `${c.summary.title}.${ext}`, messages: c.summary.messages, media_written: format === "cva" ? 12 : 0, media_failed: 0 };
        }),
        failed: [],
      };
    },
    onExportProgress: (cb) => {
      progress.add(cb);
      return () => progress.delete(cb);
    },
    pickFolder: async () => "/demo/Exports",
    closeArchive: async () => {},
    log: async () => ["INFO  cv_core: mock backend", "INFO  app: archive opened"],
    getTheme: async () => theme,
    setTheme: async (t) => {
      theme = t;
    },
    pickInput: async (kind) => (kind === "folder" ? "/demo/SignalBackups" : "/demo/WhatsApp Chat - Demo.zip"),
    pickOutput: async (name, format) => `/demo/${name}.${format === "cva" ? "zip" : format}`,
    blobUrl: (_handle, blob) => {
      const n = Number(blob.split(":")[1] ?? 0);
      return blob.startsWith("stk:") ? svg(160, 160, (n * 37) % 360, "🙂") : svg(800, 600, (n * 37) % 360, `Photo ${n}`);
    },
    onDrop: () => () => {},
  };
}
