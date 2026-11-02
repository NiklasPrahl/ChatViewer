import { useCallback, useEffect, useState } from "react";

import { type Api, api as loadApi, errorMessage } from "./api";
import { ChatList } from "./components/ChatList";
import { AboutDialog, ErrorDialog, InfoPanel, KeyDialog } from "./components/Dialogs";
import { ExportChats } from "./components/ExportChats";
import { MessageList } from "./components/MessageList";
import type { ChatInfo, OpenResult, Theme } from "./types";
import { LOCALE } from "./format";

interface Opened extends OpenResult {
  path: string;
}

export function App() {
  const [api, setApi] = useState<Api | null>(null);
  const [theme, setTheme] = useState<Theme>("auto");
  const [opened, setOpened] = useState<Opened | null>(null);
  const [chat, setChat] = useState<ChatInfo | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [keyPrompt, setKeyPrompt] = useState<{ path: string; prompt: string } | null>(null);
  const [showInfo, setShowInfo] = useState(false);
  const [showExport, setShowExport] = useState(false);
  const [showAbout, setShowAbout] = useState(false);

  useEffect(() => {
    loadApi().then(async (a) => {
      setApi(a);
      setTheme(await a.getTheme().catch(() => "auto" as Theme));
    });
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    if (theme === "auto") root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", theme);
  }, [theme]);

  const onError = useCallback((e: unknown) => setError(errorMessage(e)), []);

  const open = useCallback(
    async (path: string, secret?: string, dateOrder?: string) => {
      if (!api) return;
      setBusy(secret ? "Decrypting backup…" : "Opening…");
      try {
        const result = await api.openArchive({ path, secret, date_order: dateOrder });
        if (opened) await api.closeArchive(opened.handle);
        setChat(null);
        setOpened({ ...result, path });
      } catch (e) {
        onError(e);
      } finally {
        setBusy(null);
      }
    },
    [api, opened, onError],
  );

  const choose = useCallback(
    async (path: string) => {
      if (!api) return;
      try {
        const inspection = await api.inspect(path);
        if (!inspection.importer) {
          setError("This file or folder is not a supported chat archive. Supported: WhatsApp exports (.zip, .txt or folder), Signal backup folders, ChatViewer archives (.zip).");
        } else if (inspection.secret_prompt) {
          setKeyPrompt({ path, prompt: inspection.secret_prompt });
        } else {
          await open(path);
        }
      } catch (e) {
        onError(e);
      }
    },
    [api, open, onError],
  );

  useEffect(() => api?.onDrop((paths) => paths[0] && choose(paths[0])), [api, choose]);

  const selectChat = async (id: string) => {
    if (!api || !opened) return;
    setBusy("Loading chat…");
    try {
      setChat(await api.openChat(opened.handle, id));
    } catch (e) {
      onError(e);
    } finally {
      setBusy(null);
    }
  };

  const changeTheme = (t: Theme) => {
    setTheme(t);
    api?.setTheme(t).catch(onError);
  };

  const warnings = opened?.report.warnings ?? [];

  return (
    <div className="app">
      <header className="toolbar">
        <strong className="brand">ChatViewer</strong>
        <button onClick={async () => { const p = await api?.pickInput("file"); if (p) choose(p); }}>Open file…</button>
        <button onClick={async () => { const p = await api?.pickInput("folder"); if (p) choose(p); }}>Open folder…</button>
        <span className="spacer" />
        {opened && <button onClick={() => setShowInfo(true)}>About this archive</button>}
        {opened && <button className="primary" onClick={() => setShowExport(true)}>Export chats…</button>}
        <button onClick={() => setShowAbout(true)} aria-label="About ChatViewer">About</button>
        <label className="theme">
          <span className="sr-only">Theme</span>
          <select value={theme} onChange={(e) => changeTheme(e.target.value as Theme)} aria-label="Theme">
            <option value="auto">Auto theme</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </label>
      </header>

      {!opened ? (
        <main className="welcome">
          <div className="drop">
            <h1>Open a chat archive</h1>
            <p>Drop a file or folder here, or use the buttons above.</p>
            <ul>
              <li><strong>WhatsApp</strong>: exported chat (.zip, .txt or unpacked folder), iOS and Android</li>
              <li><strong>Signal</strong>: on-device backup folder (needs the 64-character recovery key)</li>
              <li><strong>ChatViewer archive</strong>: a .zip exported from this app</li>
            </ul>
            <p className="muted small">Everything stays on this computer. Nothing is uploaded or stored.</p>
          </div>
        </main>
      ) : (
        <div className="layout">
          <ChatList chats={opened.chats} selected={chat?.conversation.id ?? null} onSelect={selectChat} />
          <main className="chat">
            {warnings.length > 0 && (
              <div className="banner" role="status">
                ⚠️ {warnings[0]}
                {warnings.length > 1 && ` (+${warnings.length - 1} more)`}{" "}
                <button className="link" onClick={() => setShowInfo(true)}>Details</button>
              </div>
            )}
            {chat && api ? (
              <>
                <div className="chat-header">
                  <h2>{chat.conversation.title}</h2>
                  <span className="muted small">
                    {chat.message_count.toLocaleString(LOCALE)} messages · {chat.participants.length} participants
                  </span>
                </div>
                <MessageList key={`${opened.handle}-${chat.conversation.id}`} api={api} handle={opened.handle} chat={chat} onError={onError} />
              </>
            ) : (
              <div className="empty muted">Select a chat.</div>
            )}
          </main>
        </div>
      )}

      {busy && <div className="busy" role="status"><span className="spinner" /> {busy}</div>}
      {keyPrompt && (
        <KeyDialog
          prompt={keyPrompt.prompt}
          onCancel={() => setKeyPrompt(null)}
          onSubmit={(key) => { const { path } = keyPrompt; setKeyPrompt(null); open(path, key); }}
        />
      )}
      {showInfo && opened && (
        <InfoPanel
          report={opened.report}
          importer={opened.importer}
          onClose={() => setShowInfo(false)}
          onReopen={opened.importer === "whatsapp" ? (order) => { setShowInfo(false); open(opened.path, undefined, order); } : undefined}
        />
      )}
      {showExport && opened && api && (
        <ExportChats
          api={api}
          handle={opened.handle}
          chats={opened.chats}
          initial={chat ? [chat.conversation.id] : []}
          onClose={() => setShowExport(false)}
        />
      )}
      {showAbout && api && <AboutDialog getAbout={() => api.about()} onClose={() => setShowAbout(false)} />}
      {error && api && <ErrorDialog message={error} onClose={() => setError(null)} getLog={() => api.log()} />}
    </div>
  );
}
