import { useState } from "react";
import type { SessionInfo } from "../api";
import { useLayout } from "../layoutContext";
import { SessionsTable } from "../components/SessionsTable";
import { SessionDrawer } from "../components/SessionDrawer";

export function SessionsPage() {
  const { state } = useLayout();
  const [selected, setSelected] = useState<SessionInfo | null>(null);

  return (
    <>
      <SessionsTable sessions={state?.sessions ?? []} onSelect={setSelected} />
      {selected && <SessionDrawer session={selected} onClose={() => setSelected(null)} />}
    </>
  );
}
