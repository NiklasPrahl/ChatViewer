// TypeScript mirror of the neutral model's JSON (crates/cv-model).

export type Timestamp =
  | { kind: "utc_millis"; value: number }
  | { kind: "naive_local"; value: string };

export type Author = { type: "participant"; id: string } | { type: "system" };

export type TextStyle =
  | { kind: "bold" | "italic" | "strikethrough" | "monospace" | "spoiler" }
  | { kind: "mention"; value: string }
  | { kind: "link"; value: string };

export interface TextRange {
  start: number;
  len: number;
  style: TextStyle;
}

export interface RichText {
  text: string;
  ranges?: TextRange[];
}

export type MediaKind =
  | "image" | "video" | "audio" | "voice" | "gif" | "sticker" | "document" | "contact" | "other";

export type Availability =
  | { status: "available"; blob: string; integrity: "verified" | "authenticated_only" | "unchecked" }
  | { status: "omitted" }
  | { status: "missing" }
  | { status: "not_in_backup" };

export interface Attachment {
  kind: MediaKind;
  mime?: string;
  file_name?: string;
  size?: number;
  width?: number;
  height?: number;
  duration_ms?: number;
  waveform?: number[];
  blurhash?: string;
  availability: Availability;
}

export type SystemKind =
  | "encryption_notice" | "group_created" | "member_added" | "member_removed" | "member_left"
  | "member_joined" | "subject_changed" | "icon_changed" | "description_changed"
  | "security_code_changed" | "disappearing_messages" | "missed_call" | "other";

export type Content =
  | ({ type: "text" } & RichText)
  | { type: "media"; attachments: Attachment[]; caption?: RichText }
  | { type: "sticker"; emoji?: string; pack_id?: string; sticker_id?: number; image: Attachment }
  | { type: "location"; latitude: number; longitude: number; name?: string; url?: string }
  | { type: "poll"; question: string; options: { text: string; votes?: number }[] }
  | { type: "system"; kind: SystemKind; actors?: string[]; text: string }
  | { type: "deleted"; by?: string }
  | { type: "unsupported"; source_kind: string; note: string };

export interface Quote {
  target?: string;
  author?: string;
  snippet: RichText;
}

export interface Reaction {
  emoji: string;
  author: string;
  timestamp?: Timestamp;
}

export interface Message {
  id: string;
  seq: number;
  timestamp: Timestamp;
  author: Author;
  content: Content;
  quote?: Quote;
  reactions?: Reaction[];
  flags?: { edited?: boolean; forwarded?: boolean; view_once?: boolean };
  extra?: { fields?: Record<string, unknown> };
}

export interface Participant {
  id: string;
  display_name: string;
  is_self: "yes" | "no" | "unknown";
  handle?: string;
}

export type ConversationKind = "direct" | "group" | "note_to_self" | "broadcast" | "unknown";

export interface Conversation {
  id: string;
  kind: ConversationKind;
  title: string;
  participants: string[];
}

export interface SourceReport {
  app: string;
  platform: string;
  variant: string;
  source_version?: string;
  confidence: number;
  warnings?: string[];
  limitations?: string[];
  counters?: Record<string, number>;
}

export interface ChatSummary {
  id: string;
  title: string;
  kind: ConversationKind;
  participants: number;
  messages: number;
  first?: Timestamp;
  last?: Timestamp;
}

export interface OpenResult {
  handle: number;
  importer: string;
  report: SourceReport;
  chats: ChatSummary[];
}

export interface ChatInfo {
  conversation: Conversation;
  participants: Participant[];
  message_count: number;
}

export interface ExportResult {
  messages: number;
  media_written: number;
  media_failed: number;
  lossless: boolean;
}

export type ExportFormat = "txt" | "md" | "json" | "cva";
export type Theme = "auto" | "light" | "dark";

export interface BatchExportResult {
  folder: string;
  exported: { chat: string; file: string; messages: number; media_written: number; media_failed: number }[];
  failed: { chat: string; error: string }[];
}

export interface ExportProgress {
  done: number;
  total: number;
}
