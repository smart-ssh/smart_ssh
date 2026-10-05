# ADR 0100 — Eine Quelle für die Kennzeile der Drittlizenz-Datei

Status: akzeptiert
Betrifft: Spec 0099 (A1.8, A2.2, A4.3), Issue #7

## Problem

Die Kennzeile `SMART-SSH-THIRD-PARTY-NOTICES-V1` stand als eigenes Literal
im Generierungs-Skript, in der App (`ThirdPartyLicensesDialog.tsx`), in der
Prüfung des Release-Workflows und in drei Test-Fixtures. Ändert sich eine
Kopie allein, zeigt die App entweder „nur in Release-Builds" trotz
vorhandener Liste, oder der Release-Job bricht ab.

## Entscheidung

1. **Quelle ist `export const MARKER` in
   `scripts/generate-third-party-notices.mjs`** — das Skript schreibt die
   Datei, also bestimmt es die Kennzeile.
2. **Das Frontend hat genau eine Kopie**,
   `apps/smart-ssh-community/frontend/src/thirdPartyNoticesMarker.ts`
   (`THIRD_PARTY_NOTICES_MARKER`). App und alle Tests importieren sie. Sie
   bleibt eine Kopie statt eines Imports aus dem Skript, wie schon in Spec
   0099 begründet: Die App soll keine Abhängigkeit auf ein Build-Skript
   außerhalb des Frontends bekommen (und ein Import der `.mjs`-Datei bräuchte
   eigene Typdeklarationen für `tsc -b`).
3. **Der Release-Workflow behält sein Literal**, und ein Test prüft es.
   `src/thirdPartyNoticesMarker.test.ts` (läuft mit `npm test` in der
   Community-CI auf allen drei Plattformen) liest Skript und
   `release.yml` als Text und schlägt fehl, wenn
   - die Frontend-Kopie nicht gleich `MARKER` im Skript ist,
   - das Skript die Kennzeile ein zweites Mal als Literal enthält,
   - `release.yml` nicht genau eine `grep -q "^…"`-Prüfung mit genau diesem
     Wert enthält, oder irgendwo eine abweichende Kopie steht,
   - irgendeine andere Datei unter `frontend/src` die Kennzeile (auch eine
     andere Version, `SMART-SSH-THIRD-PARTY-NOTICES…`) als Literal enthält.

## Abgewogene Alternative

Der Workflow hätte die Kennzeile zur Laufzeit aus dem Skript lesen können
(`node -e "import('./scripts/…').then(m => console.log(m.MARKER))"`). Das
wurde verworfen, weil es die Quelle zwar direkt nutzt, sich aber nur im
echten Release-Lauf auf drei Betriebssystemen prüfen lässt (Pfad- und
Quoting-Eigenheiten unter Windows-Bash), und ein Fehler dort erst beim
Release auffiele. Der Test ist einfacher, läuft bei jedem PR und deckt
zusätzlich die Frontend-Kopie ab.

## Konsequenzen

- Wer die Kennzeile ändert, ändert sie im Skript, in
  `thirdPartyNoticesMarker.ts` und in `release.yml`; vergisst er eine
  Stelle, wird `npm test` rot.
- Wert und Dateiformat bleiben unverändert.
- `release.yml` selbst trägt (noch) keinen Kommentar, der auf den Test
  verweist; der Hinweis steht im Skript neben `MARKER` und in diesem ADR.
  Ein Verweis-Kommentar im Workflow wäre eine reine Ergänzung ohne
  Verhaltensänderung.
