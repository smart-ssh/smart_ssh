// Type declarations for check-doc-paths.mjs (frontend test).
export interface DocPathsConfig {
  documents?: string[];
  baseDirs?: string[];
  allowlist?: string;
}
export const KNOWN_EXTENSIONS: string[];
export const DEFAULT_CONFIG: Required<DocPathsConfig>;
export function checkDocPaths(root?: string, config?: DocPathsConfig): string[];
