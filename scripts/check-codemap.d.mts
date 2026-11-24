// Type declarations for check-codemap.mjs (frontend test).
export function parseCodemap(markdown: string): { line: number; path: string; symbols: string[] }[];
export function findCodemapProblems(markdown: string, root?: string): { line: number; message: string }[];
export function checkCodemap(opts?: { file?: string; root?: string }): { line: number; message: string }[];
