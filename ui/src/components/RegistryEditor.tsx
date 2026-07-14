import { useEffect, useState } from "react";
import { fetchBrowsersRaw, putBrowsersRaw, type BrowserInfo } from "../api";
import { RegistryView } from "./RegistryView";

export function RegistryEditor({
  browsers,
  onSaved,
}: {
  browsers: Record<string, BrowserInfo>;
  onSaved: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (editing) {
      fetchBrowsersRaw()
        .then((t) => {
          setText(t);
          setError(null);
        })
        .catch((e) => setError(String(e)));
    }
  }, [editing]);

  if (!editing) {
    return (
      <div>
        <div className="toolbar">
          <button onClick={() => setEditing(true)}>Edit browsers.json</button>
        </div>
        <RegistryView browsers={browsers} />
      </div>
    );
  }

  const save = () => {
    try {
      JSON.parse(text);
    } catch (e) {
      setError(`invalid JSON: ${e}`);
      return;
    }
    setSaving(true);
    putBrowsersRaw(text)
      .then(() => {
        setEditing(false);
        setError(null);
        onSaved();
      })
      .catch((e) => setError(String(e)))
      .finally(() => setSaving(false));
  };

  return (
    <div>
      <div className="toolbar">
        <button onClick={save} disabled={saving}>
          {saving ? "Saving…" : "Save"}
        </button>
        <button onClick={() => setEditing(false)}>Cancel</button>
      </div>
      {error && <div className="error">{error}</div>}
      <textarea
        className="json-editor"
        value={text}
        onChange={(e) => setText(e.target.value)}
        spellCheck={false}
      />
    </div>
  );
}
