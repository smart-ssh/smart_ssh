// Typdeklarationen für die Teile von generate-third-party-notices.mjs, die
// Frontend-Tests importieren (`tsc -b` prüft die Tests mit `strict`, ein
// untypisierter `.mjs`-Import wäre dort ein Fehler). Nur die exportierte
// Oberfläche, keine Implementierung.

export const MARKER: string;

export interface NpmPackageNotice {
  name: string;
  version: string;
  licenseExpr: string;
  licenseText: string;
  notice: string | null;
}

export interface NpmLsNode {
  version?: string;
  resolved?: string;
  dependencies?: Record<string, NpmLsNode>;
}

export function parseAllowList(licensesFilePath: string): string[];
export function isLicenseAllowed(expr: string, allowList: string[]): boolean;
export function collectProdPackages(npmLsTree: NpmLsNode): { name: string; version: string }[];
export function localPackagePredicate(
  npmLsTree: NpmLsNode,
): (name: string, info: NpmLsNode) => boolean;
export function collectNpmProdPackages(frontendDir: string, allowList: string[]): NpmPackageNotice[];
