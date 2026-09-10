/**
 * Spec 0055, Teil 4: löst `SettingsSectionContribution.label` auf —
 * entweder ein Übersetzungsschlüssel (z. B. `"settings.categories.
 * mcpServer"`) oder ein fester Anzeigetext (bestehende Registrierungen,
 * z. B. eine externe/private Sektion, die keinen Schlüssel im
 * `common.json` dieses Repos haben kann).
 *
 * **Unterscheidung Schlüssel vs. fester Text**: kein Konventions-Präfix
 * und kein zweites Feld — stattdessen `i18next.exists(label)`: existiert
 * IRGENDEIN geladenes Sprachpaket mit genau diesem Pfad als Schlüssel,
 * wird `label` als Schlüssel behandelt und übersetzt; sonst gilt `label`
 * unverändert als Anzeigetext. Das erfüllt die Spec-Vorgabe wörtlich
 * ("ein fester String OHNE passenden Übersetzungs-Schlüssel wird weiter
 * direkt angezeigt") mit der Registry selbst weiterhin komplett
 * i18n-unabhängig (s. `extensions/registry.ts`s Doc-Kommentar zu
 * `label`) — die Auflösung passiert erst hier beim Rendern, nicht beim
 * Registrieren. Eine Präfix-Konvention (z. B. "nur Strings mit
 * `settings.` sind Schlüssel") hätte zusätzlich eine für Registrierende
 * zu lernende Regel gebraucht; `exists()` braucht keine — jeder
 * tatsächlich im geladenen `common.json` vorhandene Pfad wird
 * automatisch erkannt, jeder andere String bleibt exakt das, was
 * übergeben wurde. Restrisiko (bewusst hingenommen): eine noch nie
 * registrierte Sektion, deren FESTER Anzeigetext zufällig exakt einem
 * bestehenden i18next-Pfad entspricht (z. B. wortwörtlich
 * `"settings.title"`), würde fälschlich übersetzt — für kurze,
 * satzartige UI-Labels praktisch ausgeschlossen.
 *
 * Eigene Datei statt in `SettingsScreen.tsx` (oxlint `react/only-export-
 * components`): eine Komponenten-Datei, die zusätzlich eine normale
 * Funktion exportiert, bricht React Fast Refresh für diese Datei.
 */
export function resolveSectionLabel(
  label: string | undefined,
  id: string,
  i18n: { exists: (key: string) => boolean },
  t: (key: string) => string,
): string {
  if (label === undefined) return id;
  return i18n.exists(label) ? t(label) : label;
}
