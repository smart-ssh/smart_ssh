// Type declarations for check-workspace-deps.mjs (frontend test).
export function parseDeps(text: string): { name: string; section: string; body: string }[];
export function workspaceDepNames(rootText: string): Set<string>;
export function checkWorkspaceDeps(root?: string): string[];
