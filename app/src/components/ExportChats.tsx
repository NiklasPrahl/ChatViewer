import { useEffect, useMemo, useRef, useState } from "react";

import type { Api } from "../api";
import { errorMessage } from "../api";
import { LOCALE, formatShortDate } from "../format";
import type { BatchExportResult, ChatSummary, ExportFormat, ExportProgress } from "../types";

const FORMATS: { id: ExportFormat; name: string; description: string }[] = [
  {
    id: "cva",
    name: "ChatViewer archive (.zip)",
    description: "One decrypted ZIP per chat with all media. Lossless; reopens in ChatViewer without the backup or key.",
  },
  { id: "json", name: "JSON", description: "Lossless data, without media files." },
  { id: "md", name: "Markdown", description: "Readable document; media are listed by name." },
  { id: "txt", name: "Plain text", description: "Simple text, one line per message." },
];

interface Props {
  api: Api;
  handle: number;
  chats: ChatSummary[];
  /** Chats to preselect (e.g. the one that is open). */
  initial: string[];
  onClose: () => void;
}

/** Export one or many chats, each into its own file in a chosen folder. */
export function ExportChats({ api, handle, chats, initial, onClose }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set(initial));
  const [query, setQuery] = useState("");
  const [format, setFormat] = useState<ExportFormat>("cva");
  const [progress, setProgress] = useState<ExportProgress | null>(null);
  const [result, setResult] = useState<BatchExportResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const running = progress !== null && result === null && error === null;

  useEffect(() => {
    if (ref.current && !ref.current.open) ref.current.showModal();
  }, []);
  useEffect(() => api.onExportProgress(setProgress), [api]);

  const titles = useMemo(() => new Map(chats.map((c) => [c.id, c.title])), [chats]);
  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? chats.filter((c) => c.title.toLowerCase().includes(q)) : chats;
  }, [chats, query]);
  const allVisibleSelected = visible.length > 0 && visible.every((c) => selected.has(c.id));

  const toggle = (id: string) =>
    setSelected((old) => {
      const next = new Set(old);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const toggleVisible = () =>
    setSelected((old) => {
      const next = new Set(old);
      for (const c of visible) {
        if (allVisibleSelected) next.delete(c.id);
        else next.add(c.id);
      }
      return next;
    });

  const run = async () => {
    const folder = await api.pickFolder();
    if (!folder) return;
    setError(null);
    setResult(null);
    // Keep the chat-list order for a predictable export.
    const ids = chats.filter((c) => selected.has(c.id)).map((c) => c.id);
    setProgress({ done: 0, total: ids.length });
    try {
      setResult(await api.exportChats(handle, ids, format, folder));
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const messages = chats.filter((c) => selected.has(c.id)).reduce((n, c) => n + c.messages, 0);

  return (
    <dialog ref={ref} className="modal wide" onCancel={(e) => { e.preventDefault(); if (!running) onClose(); }}>
      <h2>Export chats</h2>
      {result ? (
        <>
          <p className="status">
            {result.exported.length.toLocaleString(LOCALE)} {result.exported.length === 1 ? "file" : "files"} written to{" "}
            <code>{result.folder}</code>
            {format === "cva" && `, with ${result.exported.reduce((n, f) => n + f.media_written, 0).toLocaleString(LOCALE)} media files`}.
          </p>
          {result.exported.some((f) => f.media_failed > 0) && (
            <p className="muted small">Some media could not be read and are marked as missing in the archives.</p>
          )}
          {result.failed.length > 0 && (
            <section className="warnings">
              <h3>{result.failed.length} chats could not be exported</h3>
              <ul>{result.failed.map((f) => <li key={f.chat}>{titles.get(f.chat) ?? f.chat}: {f.error}</li>)}</ul>
            </section>
          )}
          <div className="buttons"><button className="primary" onClick={onClose} autoFocus>Done</button></div>
        </>
      ) : (
        <>
          <div className="export-pick">
            <div className="export-chats">
              <div className="export-toolbar">
                <input
                  className="search"
                  type="search"
                  placeholder="Search chats"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  aria-label="Search chats to export"
                  disabled={running}
                />
                <button onClick={toggleVisible} disabled={running || visible.length === 0}>
                  {allVisibleSelected ? "Select none" : query ? "Select matches" : "Select all"}
                </button>
              </div>
              <div className="export-list" role="group" aria-label="Chats">
                {visible.map((c) => (
                  <label key={c.id} className="export-item">
                    <input type="checkbox" checked={selected.has(c.id)} onChange={() => toggle(c.id)} disabled={running} />
                    <span className="chat-text">
                      <span className="chat-title">{c.title}</span>
                      <span className="chat-meta">{c.messages.toLocaleString(LOCALE)} messages · {formatShortDate(c.last)}</span>
                    </span>
                  </label>
                ))}
              </div>
            </div>
            <div className="format-list" role="radiogroup" aria-label="Format">
              {FORMATS.map((f) => (
                <label key={f.id} className={`format ${format === f.id ? "selected" : ""}`}>
                  <input type="radio" name="format" checked={format === f.id} onChange={() => setFormat(f.id)} disabled={running} />
                  <span>
                    <strong>{f.name}</strong>
                    <span className="muted small">{f.description}</span>
                  </span>
                </label>
              ))}
            </div>
          </div>
          {running && progress && (
            <div className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={progress.total} aria-valuenow={progress.done}>
              <div className="progress-bar" style={{ width: `${(progress.done / Math.max(1, progress.total)) * 100}%` }} />
              <span>Exporting {progress.done.toLocaleString(LOCALE)} of {progress.total.toLocaleString(LOCALE)} chats…</span>
            </div>
          )}
          {error && <p className="error-text">{error}</p>}
          <p className="muted small selection-note">
            {selected.size.toLocaleString(LOCALE)} chats selected ({messages.toLocaleString(LOCALE)} messages). Each becomes its own file.
          </p>
          <div className="buttons">
            <button onClick={onClose} disabled={running}>Cancel</button>
            <button className="primary" onClick={run} disabled={running || selected.size === 0}>
              {running ? "Exporting…" : "Choose folder and export…"}
            </button>
          </div>
        </>
      )}
    </dialog>
  );
}
