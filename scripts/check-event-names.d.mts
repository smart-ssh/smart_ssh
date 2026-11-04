// Type declarations for check-event-names.mjs, imported by a frontend test.

export interface NameEntry {
  key: string;
  value: string;
  line: number;
}

export interface Mismatch {
  kind: string;
  name: string;
  where: string;
}

export const NOT_CONSUMED_BY_CORE_UI: Record<string, string>;
export function parseRustNames(src: string, file?: string): NameEntry[];
export function parseFrontendNames(src: string, file?: string): NameEntry[];
export function parseLiteralListeners(src: string): { call: string; line: number }[];
export function findMismatches(input: {
  rust: NameEntry[];
  frontend: NameEntry[];
  literals: { call: string; file: string; line: number }[];
  notConsumed?: Record<string, string>;
}): Mismatch[];
export function checkRepo(root?: string): {
  problems: Mismatch[];
  counts: { rust: number; frontend: number };
};
