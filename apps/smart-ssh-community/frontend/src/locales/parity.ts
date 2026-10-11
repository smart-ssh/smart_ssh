// Hilfsfunktionen der Übersetzungstests (Spec 0024): Schlüsselparität mit
// sprachspezifischen Pluralformen und die Längenwächter-Prüfung.

export const PLURAL_CATEGORIES = ["zero", "one", "two", "few", "many", "other"] as const;

/** Sammelt jeden Blatt-Schlüsselpfad (`{ a: { b: "x" } }` -> `["a.b"]`). */
export function leafKeyPaths(value: unknown, prefix = ""): string[] {
  if (value !== null && typeof value === "object" && !Array.isArray(value)) {
    return Object.entries(value as Record<string, unknown>).flatMap(([key, child]) =>
      leafKeyPaths(child, prefix ? `${prefix}.${key}` : key),
    );
  }
  return [prefix];
}

export function leafValues(value: unknown): Map<string, string> {
  const out = new Map<string, string>();
  for (const path of leafKeyPaths(value)) {
    let cur: unknown = value;
    for (const part of path.split(".")) cur = (cur as Record<string, unknown>)[part];
    out.set(path, String(cur));
  }
  return out;
}

function splitPlural(key: string): { base: string; category: string } | null {
  for (const category of PLURAL_CATEGORIES) {
    const suffix = `_${category}`;
    if (key.endsWith(suffix)) return { base: key.slice(0, -suffix.length), category };
  }
  return null;
}

/** Plural-Basisschlüssel des Englischen: Schlüssel, die dort mit `_one` und
 * `_other` vorkommen. */
function englishPluralBases(enKeys: Set<string>): Set<string> {
  const bases = new Set<string>();
  for (const key of enKeys) {
    const p = splitPlural(key);
    if (p?.category === "one" && enKeys.has(`${p.base}_other`)) bases.add(p.base);
  }
  return bases;
}

/** Vergleicht die Schlüssel einer Sprache mit Englisch. Plural-Schlüssel
 * müssen genau die Kategorien von `Intl.PluralRules(lang)` haben, alle
 * anderen Schlüssel müssen mit Englisch übereinstimmen. Liefert Probleme als
 * lesbare Zeilen (leer = in Ordnung). */
export function keyParityProblems(lang: string, resources: unknown, english: unknown): string[] {
  const enKeys = new Set(leafKeyPaths(english));
  const bases = englishPluralBases(enKeys);
  const categories = new Intl.PluralRules(lang).resolvedOptions().pluralCategories;

  const expected = new Set<string>();
  for (const key of enKeys) {
    const p = splitPlural(key);
    if (p && bases.has(p.base)) continue;
    expected.add(key);
  }
  for (const base of bases) for (const c of categories) expected.add(`${base}_${c}`);

  const actual = new Set(leafKeyPaths(resources));
  const problems: string[] = [];
  for (const key of [...expected].sort()) if (!actual.has(key)) problems.push(`missing: ${key}`);
  for (const key of [...actual].sort()) if (!expected.has(key)) problems.push(`unexpected: ${key}`);
  return problems;
}

/** Wert, mit dem die Länge eines Schlüssels verglichen wird: der englische
 * Wert desselben Schlüssels, bei Pluralformen ohne englisches Gegenstück
 * (`_few`, `_many`, …) der von `_other`. */
function englishReference(key: string, en: Map<string, string>): string | undefined {
  const direct = en.get(key);
  if (direct !== undefined) return direct;
  const p = splitPlural(key);
  return p ? en.get(`${p.base}_other`) : undefined;
}

export const LENGTH_FACTOR = 2;
export const LENGTH_SLACK = 10;

/** Werte, die länger als `2 × len(en) + 10` sind und nicht in der
 * Ausnahmeliste stehen. */
export function lengthProblems(
  resources: unknown,
  english: unknown,
  allowed: Record<string, string> = {},
): string[] {
  const en = leafValues(english);
  const problems: string[] = [];
  for (const [key, value] of leafValues(resources)) {
    const ref = englishReference(key, en);
    if (ref === undefined || key in allowed) continue;
    const max = LENGTH_FACTOR * ref.length + LENGTH_SLACK;
    if (value.length > max) problems.push(`${key}: ${value.length} > ${max}`);
  }
  return problems;
}
