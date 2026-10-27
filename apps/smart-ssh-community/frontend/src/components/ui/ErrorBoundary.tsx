import { Component, type ErrorInfo, type ReactNode } from "react";

export interface ErrorBoundaryProps {
  /** Names the guarded part in the log line (e.g. the contribution id). */
  id: string;
  /** Subject of the log line (default "Error boundary"). */
  label?: string;
  /** Rendered after an error; default: nothing. */
  fallback?: ReactNode;
  onError?: (id: string, error: unknown, info: ErrorInfo) => void;
  /** When a value changes after a failure, the children are rendered again. */
  resetKeys?: unknown[];
  children?: ReactNode;
}

interface State {
  failed: boolean;
  resetKeys: unknown[] | undefined;
}

function keysChanged(a: unknown[] = [], b: unknown[] = []): boolean {
  return a.length !== b.length || a.some((v, i) => !Object.is(v, b[i]));
}

/**
 * Catches render, lifecycle and effect errors of its subtree (not event
 * handlers or async code). Logs once with the id; the UI never shows the
 * error payload (issue #880, Spec 0045).
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, State> {
  state: State = { failed: false, resetKeys: this.props.resetKeys };

  static getDerivedStateFromError(): Partial<State> {
    return { failed: true };
  }

  static getDerivedStateFromProps(props: ErrorBoundaryProps, state: State): Partial<State> | null {
    if (!keysChanged(state.resetKeys, props.resetKeys)) return null;
    return { failed: false, resetKeys: props.resetKeys };
  }

  componentDidCatch(error: unknown, info: ErrorInfo): void {
    console.error(`${this.props.label ?? "Error boundary"} "${this.props.id}" failed to render:`, error);
    this.props.onError?.(this.props.id, error, info);
  }

  render(): ReactNode {
    return this.state.failed ? (this.props.fallback ?? null) : this.props.children;
  }
}
