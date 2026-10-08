// Typdeklarationen für check-tauri-commands.mjs, die der Frontend-Test
// importiert (`tsc -b` prüft Tests mit `strict`). Nur die exportierte
// Oberfläche.

export interface Located {
  name: string;
  line: number;
}

export interface Mismatch {
  kind: string;
  name: string;
  where: string;
}

export function stripRustComments(src: string, options?: { blankStrings?: boolean }): string;
export function stripJsComments(src: string): string;
export function parseDefinedCommands(src: string, file?: string): Located[];
export function parseRegisteredCommands(
  rustFiles: { file: string; text: string }[],
  handlerFile?: string,
): (Located & { path: string })[];
export function parseAllowedWhileLocked(src: string, file?: string): string[];
export function parseInvokedCommands(src: string): Located[];
export function isFrontendTestFile(relPath: string): boolean;
export function findMismatches(input: {
  defined: (Located & { file: string })[];
  registered: (Located & { path: string; file: string })[];
  invoked: (Located & { file: string })[];
  allowedWhileLocked?: string[];
}): Mismatch[];
export function checkRepo(root?: string): {
  problems: Mismatch[];
  counts: { defined: number; registered: number; invoked: number };
};
