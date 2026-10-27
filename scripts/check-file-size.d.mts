// Type declarations for check-file-size.mjs, imported by the frontend test.

export const PRODUCTION_LIMIT: number;
export const TEST_LIMIT: number;
export function isCounted(rel: string): boolean;
export function isTestFile(rel: string): boolean;
export function limitFor(rel: string): number;
export function countLines(text: string): number;
export function collectSizes(root: string): Map<string, number>;
export function findProblems(sizes: Map<string, number>, allowlist: Record<string, number>): string[];
export function updateAllowlist(
  sizes: Map<string, number>,
  allowlist: Record<string, number>,
): Record<string, number>;
export function compareWithBase(
  allowlist: Record<string, number>,
  baseList: Record<string, number>,
): string[];
export function main(
  argv: string[],
  out?: { log: (m: string) => void; error: (m: string) => void },
): number;
