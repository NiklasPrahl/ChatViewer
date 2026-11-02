import { useVirtualizer } from "@tanstack/react-virtual";
import { useMemo, useRef, useState } from "react";

import { formatShortDate } from "../format";
import type { ChatSummary } from "../types";

const KIND_ICON: Record<ChatSummary["kind"], string> = {
  direct: "👤", group: "👥", note_to_self: "📝", broadcast: "📢", unknown: "💬",
};

export function ChatList({ chats, selected, onSelect }: {
  chats: ChatSummary[];
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  const [query, setQuery] = useState("");
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? chats.filter((c) => c.title.toLowerCase().includes(q)) : chats;
  }, [chats, query]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 60,
    overscan: 8,
  });

  return (
    <nav className="chat-list" aria-label="Chats">
      <input
        className="search"
        type="search"
        placeholder={`Search ${chats.length.toLocaleString()} chats`}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        aria-label="Search chats"
      />
      <div className="chat-scroll" ref={scrollRef}>
        <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
          {virtualizer.getVirtualItems().map((row) => {
            const c = filtered[row.index];
            return (
              <button
                key={c.id}
                className={`chat-item ${selected === c.id ? "active" : ""}`}
                style={{ transform: `translateY(${row.start}px)`, height: row.size }}
                onClick={() => onSelect(c.id)}
                aria-current={selected === c.id}
              >
                <span className="chat-icon" aria-hidden>{KIND_ICON[c.kind]}</span>
                <span className="chat-text">
                  <span className="chat-title">{c.title}</span>
                  <span className="chat-meta">
                    {c.messages.toLocaleString()} messages · {formatShortDate(c.last)}
                  </span>
                </span>
              </button>
            );
          })}
        </div>
        {filtered.length === 0 && <p className="muted center">No chats match.</p>}
      </div>
    </nav>
  );
}
