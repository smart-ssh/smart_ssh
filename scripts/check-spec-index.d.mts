// Type declarations for check-spec-index.mjs (frontend test).
export const INDEX_HEADER: string;
export function parseSpec(
  name: string,
  content: string,
): { number: string; title: string; status: string | null; purpose: string | null };
export function buildIndex(files: Map<string, string>): string;
export function findSpecProblems(
  files: Map<string, string>,
  index: string | null,
): { file: string; message: string }[];
export function checkSpecIndex(opts?: {
  dir?: string;
  index?: string;
  writeIndex?: boolean;
}): { file: string; message: string }[];
