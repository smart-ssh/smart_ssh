// Type declarations for check-adr.mjs (frontend test).
export const INDEX_HEADER: string;
export function parseAdr(
  name: string,
  content: string,
): { number: string; title: string; status: string | null; successor: string | null };
export function buildIndex(files: Map<string, string>): string;
export function findAdrProblems(
  files: Map<string, string>,
  index: string | null,
): { file: string; message: string }[];
export function checkAdr(opts?: {
  dir?: string;
  index?: string;
  writeIndex?: boolean;
}): { file: string; message: string }[];
