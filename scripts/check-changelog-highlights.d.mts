// Typdeklarationen für check-changelog-highlights.mjs (Frontend-Test).
export const CUTOFF_VERSION: string;
export const MIN_ITEMS: number;
export const MAX_ITEMS: number;
export function parseSections(text: string): { version: string; lines: string[] }[];
export function findHighlightProblems(
  text: string,
  cutoff?: string,
): { version: string; message: string }[];
export function checkChangelog(root?: string): { version: string; message: string }[];
