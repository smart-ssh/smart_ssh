// Typdeklarationen für check-a11y-lint-rules.mjs, die der Frontend-Test
// importiert (`tsc -b` prüft Tests mit `strict`). Nur die exportierte
// Oberfläche.

export class ParseError extends Error {}

export interface A11yRuleProblem {
  name: string;
  kind: "missing" | "severity";
  severity: unknown;
}

export interface RequiredRuleProblem {
  name: string;
  severity: string | number | undefined;
}

export const REQUIRED_ERROR_RULES: string[];
export function findRequiredRuleProblems(configText: string, required?: string[]): RequiredRuleProblem[];
export function severityOf(value: unknown): string | number | undefined;
export function parseEnabledA11yRules(printConfigText: string): string[];
export function parseConfiguredA11yRules(configText: string): Map<string, unknown>;
export function findA11yRuleProblems(input: {
  enabled: string[];
  configured: Map<string, unknown>;
}): A11yRuleProblem[];
export function runOxlintPrintConfig(frontendDir?: string): string;
export function checkA11yRules(options?: {
  frontendDir?: string;
  printConfig?: (frontendDir: string) => string;
}): { problems: A11yRuleProblem[]; requiredProblems: RequiredRuleProblem[]; enabledCount: number };
export function describeProblem(p: A11yRuleProblem): string;
