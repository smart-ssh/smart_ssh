// Type declarations for check-threat-model-note.mjs (frontend test).
export const NOTE_RE: RegExp;
export function parseSecurityPaths(text: string): string[];
export function isTestFile(file: string): boolean;
export function findThreatModelProblems(
  changedFiles: string[],
  body: string,
  prefixes: string[],
): { matches: string[]; message: string } | null;
export function loadSecurityPaths(root?: string): string[];
