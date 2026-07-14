import { useEffect, useRef, useState } from "react";
import RFB from "@novnc/novnc";
import { fetchLog, killSession, vncUrl, type SessionInfo } from "../api";

type Pane = "vnc" | "logs";

export function SessionDrawer({
  session,
  onClose,
}: {
  session: SessionInfo;
  onClose: () => void;
}) {
  const [pane, setPane] = useState<Pane>(session.vnc ? "vnc" : "logs");
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);

  const kill = () => {
    if (!confirming) {
      setConfirming(true);
      return;
    }
    killSession(session.id)
      .then(onClose)
      .catch((e) => {
        setConfirming(false);
        setError(String(e));
      });
  };

  return (
    <div className="drawer-backdrop" onClick={onClose}>
      <aside className="drawer" onClick={(e) => e.stopPropagation()}>
        <header>
          <h2 className="mono">{session.id}</h2>
          <button className="close" onClick={onClose} aria-label="close">
            ×
          </button>
        </header>
        <p className="muted">
          {session.browser} {session.version} · started {session.started}
          {session.owner && <> · {session.owner}</>}
        </p>
        <nav>
          <button
            className={pane === "vnc" ? "active" : ""}
            disabled={!session.vnc}
            onClick={() => setPane("vnc")}
          >
            VNC
          </button>
          <button className={pane === "logs" ? "active" : ""} onClick={() => setPane("logs")}>
            Logs
          </button>
          <button className="danger" onClick={kill} onBlur={() => setConfirming(false)}>
            {confirming ? "Confirm kill" : "Kill session"}
          </button>
        </nav>
        {error && <div className="error">{error}</div>}
        {pane === "vnc" && session.vnc && <VncPane sessionId={session.id} />}
        {pane === "logs" && <LogPane sessionId={session.id} available={session.logs} />}
      </aside>
    </div>
  );
}

function VncPane({ sessionId }: { sessionId: string }) {
  const target = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState("connecting…");

  useEffect(() => {
    if (!target.current) return;
    const rfb = new RFB(target.current, vncUrl(sessionId));
    rfb.scaleViewport = true;
    rfb.viewOnly = false;
    rfb.addEventListener("connect", () => setStatus(""));
    rfb.addEventListener("disconnect", () => setStatus("disconnected"));
    return () => rfb.disconnect();
  }, [sessionId]);

  return (
    <div className="vnc-pane">
      {status && <p className="muted">{status}</p>}
      <div className="vnc-canvas" ref={target} />
    </div>
  );
}

function LogPane({ sessionId, available }: { sessionId: string; available: boolean }) {
  const [text, setText] = useState<string>("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    const load = () =>
      fetchLog(sessionId)
        .then((t) => {
          if (live) {
            setText(t);
            setError(null);
          }
        })
        .catch((e) => live && setError(String(e)));
    load();
    const timer = setInterval(load, 2000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [sessionId]);

  if (!available && !text) {
    return (
      <p className="muted empty">
        {error
          ? "No log file for this session (enable with --save-all-logs or enableLog)."
          : "Waiting for log output…"}
      </p>
    );
  }
  return <pre className="log-pane">{text || "…"}</pre>;
}
