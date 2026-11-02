// Backend access. In the Tauri app this calls Rust commands; in a plain
// browser (`npm run dev:mock`) it uses synthetic data for UI work.

import type {
  BatchExportResult, ChatInfo, ExportFormat, ExportProgress, ExportResult, Message, OpenResult, Theme,
} from "./types";

export interface Inspection {
  importer: string | null;
  secret_prompt: string | null;
}

export interface OpenRequest {
  path: string;
  secret?: string;
  date_order?: string;
  self_name?: string;
}

export interface Api {
  inspect(path: string): Promise<Inspection>;
  openArchive(request: OpenRequest): Promise<OpenResult>;
  openChat(handle: number, chat: string): Promise<ChatInfo>;
  messages(handle: number, chat: string, offset: number, limit: number): Promise<Message[]>;
  messageIndex(handle: number, chat: string, message: string): Promise<number | null>;
  exportChat(handle: number, chat: string, format: ExportFormat, path: string): Promise<ExportResult>;
  /** One file per chat in `folder`; existing files are never overwritten. */
  exportChats(handle: number, chats: string[], format: ExportFormat, folder: string): Promise<BatchExportResult>;
  onExportProgress(callback: (p: ExportProgress) => void): () => void;
  /** Asks for a folder to export into; null if cancelled. */
  pickFolder(): Promise<string | null>;
  closeArchive(handle: number): Promise<void>;
  log(): Promise<string[]>;
  about(): Promise<AboutInfo>;
  getTheme(): Promise<Theme>;
  setTheme(theme: Theme): Promise<void>;
  /** Asks for an archive file or folder; null if cancelled. */
  pickInput(kind: "file" | "folder"): Promise<string | null>;
  /** Asks where to save an export; null if cancelled. */
  pickOutput(defaultName: string, format: ExportFormat): Promise<string | null>;
  /** URL for an attachment, served from memory by the backend. */
  blobUrl(handle: number, blob: string, mime?: string): string;
  /** Subscribes to files dropped on the window. */
  onDrop(callback: (paths: string[]) => void): () => void;
}

export interface AboutInfo {
  version: string;
  signal: boolean;
  third_party: string | null;
}

export const isTauri = (): boolean => "__TAURI_INTERNALS__" in window;

async function tauriApi(): Promise<Api> {
  const { invoke } = await import("@tauri-apps/api/core");
  const dialog = await import("@tauri-apps/plugin-dialog");
  const { getCurrentWebview } = await import("@tauri-apps/api/webview");
  const { listen } = await import("@tauri-apps/api/event");
  // Custom protocols are served as http://<scheme>.localhost on Windows.
  const blobBase = navigator.userAgent.includes("Windows")
    ? "http://cvblob.localhost"
    : "cvblob://localhost";
  const extensions: Record<ExportFormat, string> = {
    txt: "txt", md: "md", json: "json", cva: "zip",
  };
  return {
    inspect: (path) => invoke("inspect", { path }),
    openArchive: (request) => invoke("open_archive", { request }),
    openChat: (handle, chat) => invoke("open_chat", { handle, chat }),
    messages: (handle, chat, offset, limit) =>
      invoke("messages", { handle, chat, offset, limit }),
    messageIndex: (handle, chat, message) =>
      invoke("message_index", { handle, chat, message }),
    exportChat: (handle, chat, format, path) =>
      invoke("export_chat", { handle, chat, format, path }),
    exportChats: (handle, chats, format, folder) =>
      invoke("export_chats", { handle, chats, format, folder }),
    onExportProgress: (callback) => {
      let unlisten: (() => void) | undefined;
      let cancelled = false;
      listen<ExportProgress>("export-progress", (e) => callback(e.payload))
        .then((u) => (cancelled ? u() : (unlisten = u)));
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    pickFolder: async () => {
      const result = await dialog.open({ directory: true, multiple: false, title: "Export chats into this folder" });
      return typeof result === "string" ? result : null;
    },
    closeArchive: (handle) => invoke("close_archive", { handle }),
    log: () => invoke("log_snapshot"),
    about: () => invoke("about"),
    getTheme: async () => ((await invoke<{ theme: Theme }>("get_settings")).theme),
    setTheme: (theme) => invoke("set_settings", { settings: { theme } }),
    pickInput: async (kind) => {
      const result = await dialog.open({
        directory: kind === "folder",
        multiple: false,
        filters: kind === "file"
          ? [{ name: "Chat archives", extensions: ["zip", "txt"] }]
          : undefined,
      });
      return typeof result === "string" ? result : null;
    },
    pickOutput: async (defaultName, format) =>
      (await dialog.save({
        defaultPath: `${defaultName}.${extensions[format]}`,
        filters: [{ name: format.toUpperCase(), extensions: [extensions[format]] }],
      })) ?? null,
    blobUrl: (handle, blob, mime) =>
      `${blobBase}/${handle}/${encodeURIComponent(blob)}${
        mime ? `?mime=${encodeURIComponent(mime)}` : ""
      }`,
    onDrop: (callback) => {
      let unlisten: (() => void) | undefined;
      let cancelled = false;
      getCurrentWebview()
        .onDragDropEvent((event) => {
          if (event.payload.type === "drop") callback(event.payload.paths);
        })
        .then((u) => (cancelled ? u() : (unlisten = u)));
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
  };
}

let instance: Promise<Api> | undefined;

export function api(): Promise<Api> {
  instance ??= isTauri()
    ? tauriApi()
    : import("./mock").then((m) => m.mockApi());
  return instance;
}

/** Error text from a rejected command. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}
