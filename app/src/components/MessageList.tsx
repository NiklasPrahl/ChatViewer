import { useVirtualizer } from "@tanstack/react-virtual";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { Api } from "../api";
import { dayKey, formatDay, formatTime, nameColor } from "../format";
import type { ChatInfo, Message, Participant } from "../types";
import { MessageContent, Rich } from "./MessageContent";

const PAGE = 200;

interface Props {
  api: Api;
  handle: number;
  chat: ChatInfo;
  onError: (e: unknown) => void;
}

/** Virtualized, paged message list; only visible rows are rendered. */
export function MessageList({ api, handle, chat, onError }: Props) {
  const count = chat.message_count;
  const chatId = chat.conversation.id;
  const scrollRef = useRef<HTMLDivElement>(null);
  const [pages, setPages] = useState<Map<number, Message[]>>(new Map());
  const loading = useRef<Set<number>>(new Set());
  const [highlight, setHighlight] = useState<string | null>(null);

  const participants = useMemo(
    () => new Map<string, Participant>(chat.participants.map((p) => [p.id, p])),
    [chat.participants],
  );
  const hasSelf = chat.participants.some((p) => p.is_self === "yes");
  const group = chat.conversation.kind === "group" || chat.participants.length > 2;
  const ctx = useMemo(
    () => ({ blobUrl: (blob: string, mime?: string) => api.blobUrl(handle, blob, mime), participants }),
    [api, handle, participants],
  );

  const virtualizer = useVirtualizer({
    count,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 64,
    overscan: 12,
  });

  const at = useCallback(
    (i: number): Message | undefined => pages.get(Math.floor(i / PAGE))?.[i % PAGE],
    [pages],
  );

  const items = virtualizer.getVirtualItems();
  const first = items[0]?.index ?? 0;
  const last = items[items.length - 1]?.index ?? 0;

  // Load the pages around the visible range (and the row before it, for
  // day separators and grouping).
  useEffect(() => {
    const wanted = new Set<number>();
    for (let i = Math.max(0, first - 1); i <= last; i += PAGE) wanted.add(Math.floor(i / PAGE));
    wanted.add(Math.floor(last / PAGE));
    for (const p of wanted) {
      if (pages.has(p) || loading.current.has(p)) continue;
      loading.current.add(p);
      api
        .messages(handle, chatId, p * PAGE, PAGE)
        .then((msgs) => setPages((old) => new Map(old).set(p, msgs)))
        .catch(onError)
        .finally(() => loading.current.delete(p));
    }
  }, [first, last, pages, api, handle, chatId, onError]);

  // Start at the newest message, like a messenger.
  useEffect(() => {
    if (count > 0) virtualizer.scrollToIndex(count - 1, { align: "end" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatId, count]);

  const jumpTo = async (messageId: string) => {
    try {
      const index = await api.messageIndex(handle, chatId, messageId);
      if (index !== null) {
        virtualizer.scrollToIndex(index, { align: "center" });
        setHighlight(messageId);
        setTimeout(() => setHighlight(null), 1600);
      }
    } catch (e) {
      onError(e);
    }
  };

  return (
    <div className="messages" ref={scrollRef} role="log" aria-label="Messages">
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {items.map((row) => {
          const m = at(row.index);
          const prev = row.index > 0 ? at(row.index - 1) : undefined;
          return (
            <div
              key={row.key}
              data-index={row.index}
              ref={virtualizer.measureElement}
              className="row-wrap"
              style={{ transform: `translateY(${row.start}px)` }}
            >
              {m ? (
                <Row
                  m={m}
                  prev={prev}
                  ctx={ctx}
                  participants={participants}
                  hasSelf={hasSelf}
                  group={group}
                  highlighted={highlight === m.id}
                  onQuote={jumpTo}
                />
              ) : (
                <div className="row placeholder" />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

interface RowProps {
  m: Message;
  prev?: Message;
  ctx: Parameters<typeof MessageContent>[0]["ctx"];
  participants: Map<string, Participant>;
  hasSelf: boolean;
  group: boolean;
  highlighted: boolean;
  onQuote: (id: string) => void;
}

function Row({ m, prev, ctx, participants, hasSelf, group, highlighted, onQuote }: RowProps) {
  const newDay = !prev || dayKey(prev.timestamp) !== dayKey(m.timestamp);
  const day = newDay ? <div className="day"><span>{formatDay(m.timestamp)}</span></div> : null;

  if (m.author.type === "system" || m.content.type === "system") {
    return (
      <>
        {day}
        <div className="row system">
          <span className="system-pill">
            <MessageContent content={m.content} ctx={ctx} /> · {formatTime(m.timestamp)}
          </span>
        </div>
      </>
    );
  }

  const authorId = m.author.id;
  const author = participants.get(authorId);
  const outgoing = hasSelf && author?.is_self === "yes";
  const sameAuthorAsPrev = !newDay && prev?.author.type === "participant" && prev.author.id === authorId;
  const showName = group && !outgoing && !sameAuthorAsPrev;
  const reactions = groupReactions(m);
  const media = m.content.type === "media" || m.content.type === "sticker";

  return (
    <>
      {day}
      <div className={`row ${outgoing ? "out" : "in"} ${sameAuthorAsPrev ? "cont" : ""}`}>
        <div className={`bubble ${media ? "has-media" : ""} ${m.content.type === "sticker" ? "bare" : ""} ${highlighted ? "highlight" : ""}`}>
          {showName && (
            <div className="author" style={{ color: nameColor(authorId) }}>
              {author?.display_name ?? "Unknown"}
            </div>
          )}
          {m.quote && (
            <button
              className="quote"
              onClick={() => m.quote?.target && onQuote(m.quote.target)}
              disabled={!m.quote.target}
              title={m.quote.target ? "Show the quoted message" : "The quoted message is not in this archive"}
            >
              <span className="quote-author" style={{ color: nameColor(m.quote.author ?? "") }}>
                {participants.get(m.quote.author ?? "")?.display_name ?? "Unknown"}
              </span>
              <span className="quote-text"><Rich text={m.quote.snippet} ctx={ctx} /></span>
            </button>
          )}
          <MessageContent content={m.content} ctx={ctx} />
          <div className="meta">
            {m.flags?.view_once && <span>view once · </span>}
            {m.flags?.edited && <span>edited · </span>}
            {formatTime(m.timestamp)}
          </div>
        </div>
        {reactions.length > 0 && (
          <div className="reactions">
            {reactions.map(([emoji, n]) => (
              <span key={emoji} className="reaction">{emoji}{n > 1 ? ` ${n}` : ""}</span>
            ))}
          </div>
        )}
      </div>
    </>
  );
}

function groupReactions(m: Message): [string, number][] {
  const counts = new Map<string, number>();
  for (const r of m.reactions ?? []) counts.set(r.emoji, (counts.get(r.emoji) ?? 0) + 1);
  return [...counts];
}
