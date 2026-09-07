# 0039-error-messages-drop-technical-detail-for-next-step-guidance

## Status
Akzeptiert

## Kontext

Spec 0047, Fund D2 verlangt, dass die fünf Fehlermeldungen auf dem
Fünf-Minuten-Pfad (falscher API-Key, Modell nicht gefunden, Ollama nicht
gestartet, Host nicht erreichbar, Host-Key unbekannt/geändert) für einen
Tester verständlich sind **und** den nächsten Schritt nennen, statt roher
Technik. Der `spec-reviewer`-Durchlauf (ERHÖHT, deckte den gesamten
Commit-Bereich seit Fund A2 ab) bestätigte den Kernfund (fehlende
Handlungsanleitung, s. Commit `4857132`) und wies zusätzlich auf einen
Nebeneffekt der Umsetzung hin:

`connect_session`s `SshError` trug vor Fund D2 keinen `code` — das
Frontend zeigte den rohen `Display`-Text, der (neben dem hart-deutschen
Präfix) auch das technische Detail enthielt, z. B. `"Verbindung
fehlgeschlagen: Connection refused (os error 61)"`. Mit dem Fix ersetzt
`ServerList.tsx`s `describeError` diesen Text vollständig durch die kurze,
übersetzte Meldung samt Handlungsanleitung (`"Connection failed – is the
host reachable (address, port, network)?"`), verliert dabei aber die
Ursachenunterscheidung: `Connection refused` (Host lehnt aktiv ab, Port
falsch/Dienst nicht gestartet) vs. `No route to host`/`Network
unreachable` (Netzwerkproblem) vs. ein Timeout sehen alle identisch aus.

Zwei Optionen standen offen:

1. **Nur ersetzen** (umgesetzt): die kurze, übersetzte Meldung mit
   Handlungsanleitung ersetzt den Rohtext vollständig.
2. **Ersetzen + Detail anhängen**: dieselbe Meldung, plus eine zweite Zeile
   mit dem technischen Rohdetail (z. B. über eine `{{detail}}`-
   Interpolation in `common.json`).

## Entscheidung

Option 1 — vollständig ersetzen, kein zusätzliches Detail anzeigen.

Begründung:

- Konsistent mit den bereits vorhandenen, nie beanstandeten Meldungen
  derselben Kategorie: `AI_AUTH_FAILED`, `AI_NETWORK_ERROR`,
  `AI_PROVIDER_UNAVAILABLE` zeigen seit Spec 0024 ebenfalls nie das
  technische Rohdetail (HTTP-Body, `reqwest`-Fehlertext) — eine
  Sonderbehandlung nur für SSH-Verbindungsfehler wäre inkonsistent
  innerhalb derselben Fehlermeldungs-Familie.
- Genau das Zeigen roher, teils fremdsprachiger Technik ist der
  ursprüngliche Fund D2, den diese Spec beheben soll — ein zusätzliches
  Detail-Feld würde denselben Rohtext (nur an zweiter Stelle statt an
  erster) wieder einführen.
- Eine `{{detail}}`-Interpolation über alle Fehlerfamilien hinweg wäre ein
  neues, mehrzeiliges Meldungsformat — Aufwand außerhalb des Rahmens
  dieses Härtungs-Schritts (0047 bündelt gezielte, kleine Funde, keine
  Redesigns).

## Konsequenzen

- Ein Tester, der einen Verbindungsfehler meldet, kann aus der UI allein
  nicht mehr zwischen "Port zu, Dienst nicht erreichbar" und "Netzwerk
  down" unterscheiden — die Logdatei (Spec 0016, seit Fund B1 mit
  Startablauf-Logging) bleibt die Quelle für diese Detailtiefe, falls ein
  Tester sie meldet.
- Eine künftige Spec könnte ein einheitliches "kurze Meldung + optional
  einblendbares technisches Detail"-Muster für **alle**
  code-übersetzten Fehler einführen, statt es hier nur für SSH-Fehler
  nachzurüsten.
