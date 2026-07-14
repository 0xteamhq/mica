import { useOutletContext } from "react-router-dom";
import type { StateResponse } from "./api";

/** Shared data the Layout provides to every routed page via Outlet. */
export interface LayoutContext {
  state: StateResponse | null;
  refresh: () => void;
}

export function useLayout(): LayoutContext {
  return useOutletContext<LayoutContext>();
}
