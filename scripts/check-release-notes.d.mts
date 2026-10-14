// Type declarations for check-release-notes.mjs (frontend test).
export const LANGUAGES: string[];
export function sectionHighlights(section: { version: string; lines: string[] }): string[] | null;
export function markdownMarkup(item: string): string | null;
export function findReleaseNotesProblems(
  files: Map<string, string>,
  changelog: string,
  cutoff?: string,
): { file: string; message: string }[];
export function checkReleaseNotes(root?: string): { file: string; message: string }[];
