import { useCallback, useEffect, useState } from "react";
import { Boxes, Plus, Save } from "lucide-react";
import { fetchBrowsersRaw, putBrowsersRaw } from "../api";
import {
  addVersion,
  BROWSER_CATALOG,
  buildVersionEntry,
  firstAvailableVersion,
  parseBrowsers,
  removeVersion,
  setDefaultVersion,
} from "@/lib/browsers";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { ErrorAlert } from "./ErrorAlert";
import { RegistryView } from "./RegistryView";

type Mode = "form" | "json";

export function RegistryEditor({ onSaved }: { onSaved: () => void }) {
  const [mode, setMode] = useState<Mode>("form");
  const [text, setText] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const load = useCallback(() => {
    fetchBrowsersRaw()
      .then((t) => {
        setText(t);
        setDirty(false);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(load, [load]);

  const apply = (next: string) => {
    setText(next);
    setDirty(true);
  };

  const save = () => {
    if (text == null) return;
    try {
      JSON.parse(text);
    } catch (e) {
      setError(`invalid JSON: ${e}`);
      return;
    }
    setSaving(true);
    putBrowsersRaw(text)
      .then(() => {
        setError(null);
        setDirty(false);
        onSaved();
        load();
      })
      .catch((e) => setError(String(e)))
      .finally(() => setSaving(false));
  };

  if (text == null) return error ? <ErrorAlert message={error} /> : null;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <div className="bg-muted inline-flex rounded-md p-0.5">
          {(["form", "json"] as const).map((m) => (
            <button
              key={m}
              onClick={() => setMode(m)}
              className={cn(
                "rounded px-3 py-1 text-sm font-medium transition-colors",
                mode === m
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              {m === "form" ? "Form" : "JSON"}
            </button>
          ))}
        </div>
        <div className="ml-auto flex items-center gap-2">
          {dirty && <span className="text-muted-foreground text-xs">unsaved changes</span>}
          <Button size="sm" onClick={save} disabled={!dirty || saving}>
            <Save className="size-4" />
            {saving ? "Saving…" : "Save"}
          </Button>
          {dirty && (
            <Button variant="ghost" size="sm" onClick={load} disabled={saving}>
              Discard
            </Button>
          )}
        </div>
      </div>

      {error && <ErrorAlert message={error} />}

      {mode === "form" ? (
        <FormMode text={text} onInsert={apply} onError={setError} />
      ) : (
        <textarea
          className="border-input focus-visible:border-ring focus-visible:ring-ring/50 min-h-96 w-full resize-y rounded-md border bg-transparent p-3 font-mono text-xs shadow-xs outline-none focus-visible:ring-[3px]"
          value={text}
          onChange={(e) => apply(e.target.value)}
          spellCheck={false}
        />
      )}
    </div>
  );
}

function FormMode({
  text,
  onInsert,
  onError,
}: {
  text: string;
  onInsert: (next: string) => void;
  onError: (msg: string | null) => void;
}) {
  let browsers: Record<string, { default: string; versions: string[] }> = {};
  let parseErr: string | null = null;
  try {
    browsers = parseBrowsers(text);
  } catch (e) {
    parseErr = String(e);
  }

  const edit = (fn: () => string) => {
    try {
      onInsert(fn());
      onError(null);
    } catch (e) {
      onError(String(e));
    }
  };

  return (
    <div className="space-y-4">
      <AddVersionHelper text={text} onInsert={onInsert} onError={onError} />
      {parseErr ? (
        <ErrorAlert message={`${parseErr} — switch to JSON mode to fix it.`} />
      ) : (
        <RegistryView
          browsers={browsers}
          onSetDefault={(k, v) => edit(() => setDefaultVersion(text, k, v))}
          onRemove={(k, v) => edit(() => removeVersion(text, k, v))}
        />
      )}
    </div>
  );
}

/** Pick a suggested browser + a version from its hardcoded list and
 *  splice a valid entry into the JSON — any version added flawlessly. */
function AddVersionHelper({
  text,
  onInsert,
  onError,
}: {
  text: string;
  onInsert: (next: string) => void;
  onError: (msg: string | null) => void;
}) {
  const [idx, setIdx] = useState(0);
  const [version, setVersion] = useState(() => firstAvailableVersion(BROWSER_CATALOG[0]));
  const template = BROWSER_CATALOG[idx];
  const preview = buildVersionEntry(template, version || "<version>");

  const pickBrowser = (nextIdx: number) => {
    setIdx(nextIdx);
    setVersion(firstAvailableVersion(BROWSER_CATALOG[nextIdx]));
  };

  const add = () => {
    const chosen = template.versions.find((v) => v.version === version);
    if (!chosen || chosen.status !== "available") {
      onError("pick an available version");
      return;
    }
    try {
      onInsert(addVersion(text, template, version));
      onError(null);
    } catch (e) {
      onError(`add version: ${e}`);
    }
  };

  return (
    <div className="bg-card space-y-4 rounded-lg border p-4 shadow-sm">
      <div className="flex items-center gap-2 text-sm font-semibold">
        <Boxes className="text-muted-foreground size-4" />
        Add a version
      </div>
      <div className="flex flex-wrap items-start gap-x-6 gap-y-4">
        <div className="space-y-1.5">
          <div className="text-muted-foreground text-xs font-medium tracking-wide uppercase">
            Browser
          </div>
          <select
            className="border-input focus-visible:border-ring focus-visible:ring-ring/50 h-9 rounded-md border bg-transparent px-3 text-sm shadow-xs outline-none focus-visible:ring-[3px]"
            value={idx}
            onChange={(e) => pickBrowser(Number(e.target.value))}
          >
            {BROWSER_CATALOG.map((b, i) => (
              <option key={b.key} value={i}>
                {b.label}
              </option>
            ))}
          </select>
        </div>
        <div className="space-y-1.5">
          <div className="text-muted-foreground text-xs font-medium tracking-wide uppercase">
            Version
          </div>
          <div className="flex flex-wrap gap-1.5">
            {template.versions.map((v) => {
              const soon = v.status === "coming-soon";
              return (
                <Button
                  key={v.version}
                  type="button"
                  size="sm"
                  variant={!soon && v.version === version ? "default" : "outline"}
                  disabled={soon}
                  title={soon ? "coming soon" : undefined}
                  onClick={() => setVersion(v.version)}
                >
                  {v.version}
                  {soon && (
                    <span className="ml-1 text-[10px] tracking-wide uppercase opacity-70">
                      soon
                    </span>
                  )}
                </Button>
              );
            })}
          </div>
        </div>
      </div>
      <Separator />
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="text-muted-foreground font-mono text-xs">
          → {preview.image} · port {preview.port} · path {preview.path}
        </span>
        <Button size="sm" onClick={add}>
          <Plus className="size-4" />
          Add version
        </Button>
      </div>
    </div>
  );
}
