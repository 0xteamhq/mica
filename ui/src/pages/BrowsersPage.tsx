import { useLayout } from "../layoutContext";
import { RegistryEditor } from "../components/RegistryEditor";

export function BrowsersPage() {
  const { refresh } = useLayout();
  return <RegistryEditor onSaved={refresh} />;
}
