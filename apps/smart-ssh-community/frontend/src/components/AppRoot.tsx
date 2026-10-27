import type { ReactNode } from "react";
import { RootErrorScreen } from "./RootErrorScreen";
import { ErrorBoundary } from "./ui";

/**
 * Root of the React tree (issue #880): a render error outside every extension
 * slot shows the full-window recovery screen instead of an empty window.
 */
export function AppRoot({ children }: { children: ReactNode }) {
  return (
    <ErrorBoundary id="app-root" fallback={<RootErrorScreen />}>
      {children}
    </ErrorBoundary>
  );
}
