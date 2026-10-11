# Spec 0037 — Entitlements und Editionen

Status: umgesetzt
Zweck: Das Vokabular für Funktionen, die abhängig von der Edition freigeschaltet werden: welche Funktionen es gibt, wie ein gesperrter Aufruf scheitert, wie Regelherkunft die Filter-Reihenfolge ergänzt und welche Typen der Kern dafür kennt. Die eigentlichen Bezahlfunktionen sind nicht Teil dieses Repos.
Bezüge: Spec 0002 (Filter-Engine, Regelherkunft), Spec 0009 (erklärte Auswertung), Spec 0012 (Dokumente), Spec 0034 (Chat-Sitzungen), Spec 0038 (Verdrahtung und Registry).

## 1. Ziel

Der Kern kennt die **Typen** für editionsabhängige Funktionen, aber keine
Entitlement-**Logik** im Sinne von „was darf ein bestimmter Nutzer". Welche
Berechtigungen gelten, entscheidet allein die jeweilige Edition, die sie
bereitstellt. Die Community Edition liefert einen festen Satz, in dem keine
gesperrte Funktion freigeschaltet ist.

## 2. Funktionen und Berechtigungen

- **Funktionen** (`Feature`): SharedInventory, SharedNotes,
  CuratedRulePacks, OrgPolicy, MultiServerActions, ManagedAi, OrgAiPolicy,
  TeamAgents, SessionHistory, ActivityReport, SessionHandover, CloudSync,
  DocumentExport, AuditExport, Sso, SelfHosted. Die Namen werden als
  `snake_case` serialisiert.
- **Stufen** (`Tier`): Free, Personal, Pro, Team, Business, Enterprise.
- **Berechtigungen** (`Entitlements`): Stufe, Menge der freigeschalteten
  Funktionen, optionale Platzzahl, optionales Ablaufdatum, Kennzeichen für
  nichtkommerzielle Nutzung, optionaler Lizenznehmer. Der Ausgangswert
  „Free" hat keine gesperrte Funktion freigeschaltet.
- Eine Abfrage `has` liefert, ob eine Funktion freigeschaltet ist;
  `require` liefert denselben Zustand als Fehler `FeatureLocked` (mit
  Funktion und Stufe).
- Ein Anbieter von Berechtigungen liefert den aktuellen Stand und meldet
  Änderungen. Die Community Edition und die Tests nutzen einen festen
  Anbieter.
- `FeatureLocked` ist im Fehlertyp der Befehle strukturiert eingebettet,
  nicht als Text, damit das Frontend ihn eindeutig von fachlichen Fehlern
  unterscheidet.
- Zertifikats-Authentifizierung ist **keine** gesperrte Funktion; sie bleibt
  für alle frei.

## 3. Gating-Konvention (D5)

Jeder Befehl, der eine gesperrte Funktion auslöst, prüft die Berechtigung
**als Erstes**, bevor irgendetwas geschieht. Gesperrte Funktionen schlagen
**geschlossen** fehl: mit `FeatureLocked`, nie mit einer stillen Rückfall-
Verhaltensweise der freien Stufe.

## 4. Kein Word-Export in dieser Edition

Der Export von KI-Dokumenten (Spec 0012) kennt nur Markdown, vollständig frei.
Ein Export als Word-Datei, ein entsprechender Button und die zugehörige
Abhängigkeit existieren in diesem Repo nicht. Der Funktionsname
`DocumentExport` bleibt im Vokabular, wird aber hier nirgends geprüft, weil
es nichts gibt, das er sperren könnte. Eine spätere Edition bringt ihre
eigene Prüfung mit.

## 5. Policy-Ebenen in der Filter-Engine

Jede Regel hat eine **Herkunft**: `Builtin` (harte Sperrliste), `Organization`
(von einer Organisation bereitgestellt, lokal nicht übersteuerbar) oder
`User` (vom Nutzer definiert, im lokalen Regelspeicher).

Die Auswertungsreihenfolge (Ergänzung zu Spec 0002) ist:

1. Die harte Sperrliste (`Builtin`) wird zuerst geprüft.
2. **Aktions-Stufe:** Deny vor Confirm vor Allow vor Vorgabe — unverändert
   und unabhängig von der Herkunft. Eine Deny-Regel schlägt jede
   Allow-Regel, ob Organisation oder Nutzer.
3. **Innerhalb derselben Aktions-Stufe:** `Organization` vor `User`. Passen
   im selben Tier eine Organisations- und eine Nutzerregel auf dasselbe
   Kommando, gewinnt die Organisationsregel — **unabhängig** davon, welche die
   spezifischere Server-, Tag- oder Global-Zuordnung hat.
4. Erst danach: Spezifität des Geltungsbereichs (Server vor Tag vor
   Global), dann Priorität.

Die erklärte Auswertung (Spec 0009) nennt zusätzlich die Herkunft der
greifenden Regel. Regeln kommen über Quellen mit einer Herkunft; der lokale
Regelspeicher ist die einzige `User`-Quelle. Der Kern liefert selbst keine
`Organization`-Quelle; in der Community Edition gibt es nur den lokalen
Regelspeicher, ohne Sonderbehandlung in der Filter-Logik.

## 6. Synchronisations-Schnittstelle

Der Kern kennt eine Schnittstelle zum Hochladen und Abholen eines
verschlüsselten Pakets (`push`, `pull`), aber **keine** Umsetzung. Der
KI-Anbieter (Spec 0006) bleibt unverändert; ein später dazukommender
verwalteter Anbieter unterliegt derselben Schwärzung vor jedem Versand.

## 7. Sitzungs-Modell

Der Kern kennt ein Sitzungs-Modell mit Kennung, Server, Herkunft (Mensch
oder MCP-Agent), Start und Ende, redigierten Nachrichten, einem wörtlichen,
nie kompaktierten Protokoll (je Eintrag: Zeit, Kommando, Entscheidung,
greifende Regel, Exit-Code, Ausgabe-Prüfsumme), einer Zusammenfassung und
dem Kompaktierungs-Zustand. Die Chat-Sitzungs-Persistenz (Spec 0034)
benutzt diese Typen nicht zwingend; sie sind das Zielbild für spätere
Funktionen.

## 8. (entfallen)

Der frühere Abgleich der Chat-Sitzungen mit dem Zielbild war ein
einmaliger Prüfauftrag, kein Verhalten. Die Nummer wird nicht neu vergeben.

## 9. Sicherheitszusagen

- Keine Entitlement-Prüfung umgeht oder ersetzt die Filter-Engine; ein
  gesperrter Aufruf ändert nie das Ergebnis der Auswertung.
- `Organization`-Regeln sind durch `User`-Regeln nicht aufhebbar — auch
  nicht durch eine Nutzerregel mit spezifischerem Geltungsbereich.
- Gesperrte Funktionen schlagen geschlossen fehl, ohne stille Degradierung.

## 10. Nachweis (Testfälle)

- Ein fester Anbieter ist in allen Tests nutzbar.
- Allow (User) gegen Deny (Organization) und Allow (User) gegen Confirm
  (Organization), jeweils mit **höherer** Spezifität auf Nutzerseite: Die
  Organisationsregel gewinnt.
- Die erklärte Auswertung nennt die Herkunft der greifenden Regel.
- Der Markdown-Export läuft unabhängig vom Berechtigungszustand durch.

## 11. Grenzen

- Eine `Organization`-Quelle, eine Berechtigungs-Prüfung gegen eine Lizenz
  und eine Synchronisations-Umsetzung gehören nicht zu diesem Repo.
