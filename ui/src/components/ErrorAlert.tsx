import { TriangleAlert } from "lucide-react";

/** Inline error banner, styled with the destructive token. */
export function ErrorAlert({ message }: { message: string }) {
  return (
    <div className="border-destructive/30 bg-destructive/10 text-destructive flex items-start gap-2 rounded-md border px-3 py-2 text-sm">
      <TriangleAlert className="mt-0.5 size-4 shrink-0" />
      <span className="break-words">{message}</span>
    </div>
  );
}
