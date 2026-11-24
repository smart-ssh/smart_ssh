import type { Scope } from "../../api/types";

export type ScopeKind = "global" | "server" | "tag";

export function scopeKind(scope: Scope): ScopeKind {
  if (scope === "Global") return "global";
  if ("Server" in scope) return "server";
  return "tag";
}

export function scopeServerId(scope: Scope): string | null {
  return typeof scope === "object" && "Server" in scope ? scope.Server : null;
}

export function scopeTag(scope: Scope): string | null {
  return typeof scope === "object" && "Tag" in scope ? scope.Tag : null;
}

export function scopeKey(scope: Scope): string {
  const kind = scopeKind(scope);
  if (kind === "server") return `server:${scopeServerId(scope)}`;
  if (kind === "tag") return `tag:${scopeTag(scope)}`;
  return "global";
}
