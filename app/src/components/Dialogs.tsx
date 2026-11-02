import { Fragment, useEffect, useRef, useState } from "react";

import type { SourceReport } from "../types";
import { LOCALE } from "../format";

function Modal({ title, onClose, children, wide }: {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  return (
    <dialog ref={ref} className={`modal ${wide ? "wide" : ""}`} onCancel={(e) => { e.preventDefault(); onClose(); }}>
      <h2>{title}</h2>
      {children}
    </dialog>
  );
}

/** Normalizes the key like the backend does, for the length counter only. */
function keyLength(value: string): number {
  return value.replace(/[\s-]/g, "").length;
}

export function KeyDialog({ prompt, onSubmit, onCancel }: {
  prompt: string;
  onSubmit: (key: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState("");
  const [show, setShow] = useState(false);
  const n = keyLength(value);
  const submit = () => {
    const key = value;
    setValue(""); // do not keep the key in UI state
    onSubmit(key);
  };
  return (
    <Modal title="Unlock backup" onClose={() => { setValue(""); onCancel(); }}>
      <p>Enter the {prompt}. It is only used to decrypt the backup in memory and is never stored.</p>
      <form onSubmit={(e) => { e.preventDefault(); if (n === 64) submit(); }}>
        <input
          className="key-input"
          type={show ? "text" : "password"}
          autoFocus
          autoComplete="off"
          spellCheck={false}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          aria-label="Recovery key"
          placeholder="xxxx xxxx xxxx …"
        />
        <div className="key-row">
          <label><input type="checkbox" checked={show} onChange={(e) => setShow(e.target.checked)} /> Show key</label>
          <span className={n === 64 ? "ok" : "muted"}>{n} / 64 characters</span>
        </div>
        <div className="buttons">
          <button type="button" onClick={() => { setValue(""); onCancel(); }}>Cancel</button>
          <button type="submit" className="primary" disabled={n !== 64}>Unlock</button>
        </div>
      </form>
    </Modal>
  );
}

export function ErrorDialog({ message, onClose, getLog }: {
  message: string;
  onClose: () => void;
  getLog: () => Promise<string[]>;
}) {
  const [copied, setCopied] = useState<"idle" | "done" | "failed">("idle");
  const copy = async () => {
    try {
      const log = await getLog();
      // The log is redacted by the backend: no message content, no names.
      await navigator.clipboard.writeText([`Error: ${message}`, "", ...log].join("\n"));
      setCopied("done");
    } catch {
      setCopied("failed");
    }
  };
  return (
    <Modal title="Something went wrong" onClose={onClose}>
      <p className="error-text">{message}</p>
      <p className="muted small">The log contains no message content or names.</p>
      <div className="buttons">
        <button onClick={copy}>{copied === "done" ? "Log copied" : copied === "failed" ? "Copy failed" : "Copy log"}</button>
        <button className="primary" onClick={onClose} autoFocus>Close</button>
      </div>
    </Modal>
  );
}

export function InfoPanel({ report, importer, onClose, onReopen }: {
  report: SourceReport;
  importer: string;
  onClose: () => void;
  onReopen?: (dateOrder: string) => void;
}) {
  const dateWarning = report.warnings?.some((w) => w.includes("date order"));
  return (
    <Modal title="About this archive" onClose={onClose} wide>
      <dl className="facts">
        <dt>Source</dt><dd>{report.app} ({report.platform})</dd>
        <dt>Format</dt><dd><code>{report.variant}</code></dd>
        {report.source_version && (<><dt>Version</dt><dd>{report.source_version}</dd></>)}
        <dt>Importer</dt><dd>{importer}</dd>
        <dt>Confidence</dt><dd>{report.confidence} / 100</dd>
      </dl>
      {(report.warnings?.length ?? 0) > 0 && (
        <section className="warnings">
          <h3>Warnings</h3>
          <ul>{report.warnings!.map((w) => <li key={w}>{w}</li>)}</ul>
          {dateWarning && onReopen && (
            <div className="buttons left">
              <span className="muted small">Dates look wrong? Reopen as:</span>
              {["dmy", "mdy", "ymd"].map((o) => (
                <button key={o} onClick={() => onReopen(o)}>{o.toUpperCase()}</button>
              ))}
            </div>
          )}
        </section>
      )}
      {(report.limitations?.length ?? 0) > 0 && (
        <section>
          <h3>What this source does not contain</h3>
          <ul>{report.limitations!.map((l) => <li key={l}>{l}</li>)}</ul>
        </section>
      )}
      {report.counters && (
        <section>
          <h3>Counts</h3>
          <dl className="facts counters">
            {Object.entries(report.counters).map(([k, v]) => (
              <Fragment key={k}><dt>{k.replace(/_/g, " ")}</dt><dd>{v.toLocaleString(LOCALE)}</dd></Fragment>
            ))}
          </dl>
        </section>
      )}
      <div className="buttons"><button className="primary" onClick={onClose} autoFocus>Close</button></div>
    </Modal>
  );
}

const REPOSITORY = "https://github.com/NiklasPrahl/ChatViewer";

export function AboutDialog({ getAbout, onClose }: {
  getAbout: () => Promise<{ version: string; signal: boolean; third_party: string | null }>;
  onClose: () => void;
}) {
  const [info, setInfo] = useState<{ version: string; signal: boolean; third_party: string | null } | null>(null);
  useEffect(() => {
    getAbout().then(setInfo).catch(() => setInfo(null));
  }, [getAbout]);
  return (
    <Modal title="About ChatViewer" onClose={onClose} wide>
      <p>
        Version {info?.version ?? "…"}. An offline viewer for exported chats: nothing is uploaded,
        and chats, media and keys are never stored by the app.
      </p>
      <p className="small">
        ChatViewer's own code is licensed under the MIT License.{" "}
        {info?.signal && (
          <>
            This build includes the Signal importer, which uses Signal's AGPL-3.0 backup format
            definitions; the application as a whole is therefore distributed under the GNU Affero
            General Public License v3.0. The license texts are included with the app.{" "}
          </>
        )}
        Source code: <code>{REPOSITORY}</code>
      </p>
      <h3>Third-party software</h3>
      {info?.third_party ? (
        <pre className="licenses">{info.third_party}</pre>
      ) : (
        <p className="muted small">The full list of third-party licenses is included in release builds.</p>
      )}
      <div className="buttons"><button className="primary" onClick={onClose} autoFocus>Close</button></div>
    </Modal>
  );
}
