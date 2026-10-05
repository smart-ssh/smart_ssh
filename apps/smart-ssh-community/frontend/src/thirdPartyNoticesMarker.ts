// Spec 0099, A1.8/A4.3 (Issue #7): die Kennzeile, mit der die erzeugte
// Drittlizenz-Datei beginnt. Die maßgebliche Quelle ist `MARKER` in
// scripts/generate-third-party-notices.mjs (das Skript schreibt die Datei).
// Dies ist die einzige Kopie im Frontend — App und Tests importieren sie
// von hier. Sie bleibt eine Kopie statt eines Imports, damit die App keine
// Abhängigkeit auf ein Build-Skript außerhalb des Frontends bekommt;
// `thirdPartyNoticesMarker.test.ts` prüft sie (und die Prüfung in
// `.github/workflows/release.yml`) gegen das Skript.
export const THIRD_PARTY_NOTICES_MARKER = "SMART-SSH-THIRD-PARTY-NOTICES-V1";
