import { type ReactNode, useState } from "react";

import { formatDuration, formatSize, segments } from "../format";
import type { Attachment, Content, Participant, RichText } from "../types";

interface Ctx {
  blobUrl: (blob: string, mime?: string) => string;
  participants: Map<string, Participant>;
}

export function Rich({ text, ctx }: { text: RichText; ctx: Ctx }) {
  return (
    <span className="rich">
      {segments(text).map((s, i) => {
        let node: ReactNode = s.text;
        for (const style of s.styles) {
          switch (style.kind) {
            case "bold": node = <strong>{node}</strong>; break;
            case "italic": node = <em>{node}</em>; break;
            case "strikethrough": node = <s>{node}</s>; break;
            case "monospace": node = <code>{node}</code>; break;
            case "spoiler": node = <span className="spoiler" tabIndex={0}>{node}</span>; break;
            case "mention":
              node = <span className="mention">@{ctx.participants.get(style.value)?.display_name ?? node}</span>;
              break;
            case "link": node = <span className="url">{node}</span>; break;
          }
        }
        // URLs are shown but not opened: the app never goes online.
        if (s.url) node = <span className="url" title="Links are not opened (offline viewer)">{node}</span>;
        return <span key={i}>{node}</span>;
      })}
    </span>
  );
}

const KIND_ICON: Record<Attachment["kind"], string> = {
  image: "🖼", video: "🎞", audio: "🎵", voice: "🎤", gif: "🎞",
  sticker: "🏷", document: "📄", contact: "👤", other: "📎",
};

const KIND_LABEL: Record<Attachment["kind"], string> = {
  image: "Photo", video: "Video", audio: "Audio", voice: "Voice message", gif: "GIF",
  sticker: "Sticker", document: "Document", contact: "Contact", other: "Media",
};

function unavailableText(a: Attachment): string {
  switch (a.availability.status) {
    case "omitted": return "not included in the export";
    case "missing": return "file missing from the archive";
    case "not_in_backup": return "not in the backup";
    default: return "";
  }
}

function Waveform({ samples }: { samples: number[] }) {
  const w = 180;
  const step = w / samples.length;
  return (
    <svg className="waveform" width={w} height="28" aria-hidden>
      {samples.map((v, i) => {
        const h = Math.max(2, (v / 255) * 26);
        return <rect key={i} x={i * step} y={14 - h / 2} width={Math.max(1, step - 1)} height={h} rx="1" />;
      })}
    </svg>
  );
}

/** Plays natively; if the WebView cannot decode the format (e.g. Ogg
 * Opus voice notes in WKWebView), asks the backend for a WAV version. */
function AudioPlayer({ src, mime }: { src: string; mime?: string }) {
  const [wav, setWav] = useState(false);
  const [failed, setFailed] = useState(false);
  const canConvert = !mime || mime.includes("ogg") || mime.includes("opus");
  const url = wav ? `${src}${src.includes("?") ? "&" : "?"}as=wav` : src;
  if (failed) return <span className="note">This audio format cannot be played.</span>;
  return (
    <audio
      key={url}
      src={url}
      controls
      preload="metadata"
      onError={() => (!wav && canConvert ? setWav(true) : setFailed(true))}
    />
  );
}

export function AttachmentView({ a, ctx }: { a: Attachment; ctx: Ctx }) {
  const available = a.availability.status === "available";
  const src = a.availability.status === "available" ? ctx.blobUrl(a.availability.blob, a.mime) : "";
  const ratio = a.width && a.height ? `${a.width} / ${a.height}` : undefined;
  const isVideoMime = a.mime?.startsWith("video/");

  if (!available) {
    return (
      <div className="file-card unavailable">
        <span className="file-icon" aria-hidden>{KIND_ICON[a.kind]}</span>
        <span>
          <span className="file-name">{a.file_name ?? KIND_LABEL[a.kind]}</span>
          <span className="file-meta">
            {KIND_LABEL[a.kind]} {unavailableText(a)}
            {a.size ? ` · ${formatSize(a.size)}` : ""}
            {a.duration_ms ? ` · ${formatDuration(a.duration_ms)}` : ""}
          </span>
        </span>
      </div>
    );
  }
  switch (a.kind) {
    case "image":
      return <img className="media" src={src} alt={a.file_name ?? "Photo"} loading="lazy" style={{ aspectRatio: ratio }} />;
    case "gif":
      return isVideoMime
        ? <video className="media" src={src} autoPlay loop muted playsInline style={{ aspectRatio: ratio }} />
        : <img className="media" src={src} alt="GIF" loading="lazy" style={{ aspectRatio: ratio }} />;
    case "video":
      return <video className="media" src={src} controls preload="metadata" style={{ aspectRatio: ratio }} />;
    case "audio":
    case "voice":
      return (
        <div className="audio">
          {a.waveform && a.waveform.length > 0 && <Waveform samples={a.waveform} />}
          <AudioPlayer src={src} mime={a.mime} />
        </div>
      );
    default:
      return (
        <div className="file-card">
          <span className="file-icon" aria-hidden>{KIND_ICON[a.kind]}</span>
          <span>
            <span className="file-name">{a.file_name ?? KIND_LABEL[a.kind]}</span>
            <span className="file-meta">{[a.mime, formatSize(a.size)].filter(Boolean).join(" · ")}</span>
          </span>
        </div>
      );
  }
}

export function MessageContent({ content, ctx }: { content: Content; ctx: Ctx }) {
  switch (content.type) {
    case "text":
      return <Rich text={content} ctx={ctx} />;
    case "media":
      return (
        <>
          {content.attachments.length === 0 && <div className="note">View-once media (already viewed)</div>}
          {content.attachments.map((a, i) => <AttachmentView key={i} a={a} ctx={ctx} />)}
          {content.caption && <div className="caption"><Rich text={content.caption} ctx={ctx} /></div>}
        </>
      );
    case "sticker":
      return content.image.availability.status === "available"
        ? <img className="sticker" src={ctx.blobUrl(content.image.availability.blob, content.image.mime)} alt={content.emoji ?? "Sticker"} loading="lazy" />
        : <div className="sticker-fallback"><span className="sticker-emoji">{content.emoji ?? "🏷"}</span><span className="note">Sticker</span></div>;
    case "location":
      return (
        <div className="file-card">
          <span className="file-icon" aria-hidden>📍</span>
          <span>
            <span className="file-name">{content.name ?? "Location"}</span>
            <span className="file-meta">
              {content.latitude.toFixed(5)}, {content.longitude.toFixed(5)} · map not shown (offline)
            </span>
          </span>
        </div>
      );
    case "poll": {
      const max = Math.max(1, ...content.options.map((o) => o.votes ?? 0));
      return (
        <div className="poll">
          <div className="poll-question">📊 {content.question}</div>
          {content.options.map((o, i) => (
            <div key={i} className="poll-option">
              <div className="poll-bar" style={{ width: `${((o.votes ?? 0) / max) * 100}%` }} />
              <span>{o.text}</span>
              <span className="poll-votes">{o.votes ?? ""}</span>
            </div>
          ))}
        </div>
      );
    }
    case "deleted":
      return <span className="note">🚫 This message was deleted</span>;
    case "unsupported":
      return <span className="note">{content.note}</span>;
    case "system":
      return <span>{content.text}</span>;
  }
}
