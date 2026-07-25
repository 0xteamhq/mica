import { type ReactNode, useEffect, useMemo, useState } from "react";
import { ChevronLeft, ChevronRight, Search, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Combobox } from "@/components/ui/combobox";
import { Input } from "@/components/ui/input";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

const ANY = "__any__";

/** A single column: how to render the header and each row's cell. */
export interface Column<T> {
  /** Stable key (also the header cell's React key). */
  key: string;
  header: ReactNode;
  cell: (row: T) => ReactNode;
  /** Extra classes for both the header and body cells (e.g. alignment). */
  className?: string;
}

/** A dropdown filter: distinct values of `accessor` become the options. */
export interface Filter<T> {
  key: string;
  label: string;
  accessor: (row: T) => string;
}

export interface DataTableProps<T> {
  rows: T[];
  columns: Column<T>[];
  rowKey: (row: T) => string;
  /** Dropdown filters rendered in the filter bar. */
  filters?: Filter<T>[];
  /** When set, a search box filters rows by this text (case-insensitive). */
  searchAccessor?: (row: T) => string;
  searchPlaceholder?: string;
  pageSize?: number;
  onRowClick?: (row: T) => void;
  /** Message when there are zero rows at all. */
  empty?: ReactNode;
  /** Message when filters/search exclude every row. */
  noMatch?: ReactNode;
}

/**
 * Generic, self-contained table with a filter bar (search + dropdown
 * filters), a result counter, and client-side pagination. Owns all
 * filter/search/page state internally — callers just supply data,
 * column definitions, and (optionally) filters. Reusable for any row
 * type `T`.
 */
export function DataTable<T>({
  rows,
  columns,
  rowKey,
  filters = [],
  searchAccessor,
  searchPlaceholder = "Search",
  pageSize = 10,
  onRowClick,
  empty = "No data.",
  noMatch = "No rows match these filters.",
}: DataTableProps<T>) {
  const [selections, setSelections] = useState<Record<string, string>>({});
  const [q, setQ] = useState("");
  const [page, setPage] = useState(1);

  // Distinct sorted options for each filter, derived from the rows.
  const options = useMemo(() => {
    const map: Record<string, string[]> = {};
    for (const f of filters) {
      const set = new Set<string>();
      for (const r of rows) {
        const v = f.accessor(r);
        if (v) set.add(v);
      }
      map[f.key] = [...set].sort();
    }
    return map;
  }, [rows, filters]);

  const filtered = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return rows.filter((r) => {
      for (const f of filters) {
        const sel = selections[f.key];
        if (sel && sel !== ANY && f.accessor(r) !== sel) return false;
      }
      if (needle && searchAccessor && !searchAccessor(r).toLowerCase().includes(needle)) {
        return false;
      }
      return true;
    });
  }, [rows, filters, selections, q, searchAccessor]);

  // A narrowed result set can leave `page` past the end — reset on change.
  useEffect(() => setPage(1), [selections, q]);

  const pageCount = Math.max(1, Math.ceil(filtered.length / pageSize));
  const current = Math.min(page, pageCount);
  const pageRows = filtered.slice((current - 1) * pageSize, current * pageSize);

  const filtersActive =
    q.trim() !== "" || filters.some((f) => selections[f.key] && selections[f.key] !== ANY);
  const clear = () => {
    setSelections({});
    setQ("");
  };

  if (rows.length === 0) {
    return (
      <Card className="text-muted-foreground items-center py-12 text-center text-sm">{empty}</Card>
    );
  }

  return (
    <div className="space-y-4">
      {(searchAccessor || filters.length > 0) && (
        <div className="flex flex-wrap items-center gap-2">
          {searchAccessor && (
            <div className="relative">
              <Search className="text-muted-foreground pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2" />
              <Input
                placeholder={searchPlaceholder}
                className="w-56 pl-8"
                value={q}
                onChange={(e) => setQ(e.target.value)}
              />
            </div>
          )}
          {filters.map((f) => (
            <Combobox
              key={f.key}
              value={selections[f.key] ?? ANY}
              disabled={(options[f.key] ?? []).length === 0}
              onChange={(v) => setSelections((s) => ({ ...s, [f.key]: v }))}
              searchPlaceholder={`Search ${f.label.toLowerCase()}…`}
              options={[
                { value: ANY, label: `${f.label}: any` },
                ...(options[f.key] ?? []).map((o) => ({ value: o, label: o })),
              ]}
            />
          ))}
          {filtersActive && (
            <Button variant="ghost" size="sm" onClick={clear}>
              <X className="size-4" />
              Clear
            </Button>
          )}
          <span className="text-muted-foreground ml-auto text-xs">
            {filtered.length} of {rows.length}
          </span>
        </div>
      )}

      <Card className="overflow-hidden py-0">
        <Table>
          <TableHeader>
            <TableRow>
              {columns.map((c) => (
                <TableHead key={c.key} className={c.className}>
                  {c.header}
                </TableHead>
              ))}
            </TableRow>
          </TableHeader>
          <TableBody>
            {filtered.length === 0 ? (
              <TableRow>
                <TableCell
                  colSpan={columns.length}
                  className="text-muted-foreground py-10 text-center"
                >
                  {noMatch}
                </TableCell>
              </TableRow>
            ) : (
              pageRows.map((r) => (
                <TableRow
                  key={rowKey(r)}
                  className={cn(onRowClick && "cursor-pointer")}
                  onClick={onRowClick ? () => onRowClick(r) : undefined}
                >
                  {columns.map((c) => (
                    <TableCell key={c.key} className={c.className}>
                      {c.cell(r)}
                    </TableCell>
                  ))}
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </Card>

      {filtered.length > pageSize && (
        <div className="flex items-center justify-between text-sm">
          <span className="text-muted-foreground">
            {(current - 1) * pageSize + 1}–{Math.min(current * pageSize, filtered.length)} of{" "}
            {filtered.length}
          </span>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={current <= 1}
              onClick={() => setPage((p) => Math.max(1, p - 1))}
            >
              <ChevronLeft className="size-4" />
              Prev
            </Button>
            <span className="text-muted-foreground tabular-nums">
              Page {current} of {pageCount}
            </span>
            <Button
              variant="outline"
              size="sm"
              disabled={current >= pageCount}
              onClick={() => setPage((p) => Math.min(pageCount, p + 1))}
            >
              Next
              <ChevronRight className="size-4" />
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
