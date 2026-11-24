// Type declarations for check-conventions.mjs (frontend test).
export const EXISTING_TESTS_RS: string[];
export const GUARDED_DIRS: string[];
export const FORBIDDEN_IDENTIFIERS: string[];
export function checkConventions(root?: string, existing?: string[]): string[];
