# Spec 0056 — Provider-Key-Validierung und Gestaltung des Anbieter-Formulars

Status: umgesetzt
Zweck: Im Anbieter-Formular erkennt der Nutzer früh, ob ein API-Key zum
Anbieter passt und funktioniert, und das Formular ist klar gegliedert.
Bezüge: Spec 0006/0007/0025 (KI-Anbieter), Spec 0050 (Einstellungen,
zweispaltige Struktur), Spec 0049 (Trim der Zugangsdaten), Spec 0008
(Verbindungstest als Vorbild).

## Teil 1: Format-Hinweis (Warnung, keine Blockade)

Beim Eintragen eines API-Keys erscheint sofort ein offline berechneter
Hinweis, damit ein falscher Key oder Anbieter nicht erst beim ersten Chat
auffällt.

- Präfix je Anbieter: Anthropic `sk-ant-`, OpenAI `sk-` bzw. `sk-proj-`,
  OpenRouter `sk-or-`.
- Passt das Präfix nicht, erscheint ein Hinweis („Das sieht nicht wie ein
  Anthropic-Key aus — erwartet `sk-ant-…`. Trotzdem speichern?").
- **Warnung, nie Blockade:** Anbieter ändern Key-Formate; eine zu strenge
  Prüfung würde gültige Keys ablehnen. Der Nutzer kann immer speichern.
- Für generische OpenAI-kompatible Anbieter gibt es keine
  Format-Prüfung (kein vorhersagbares Format).
- Die Prüfung läuft rein offline im Frontend und ergänzt den Trim der
  Eingabe (Spec 0049), der die eigentliche Whitespace-Lösung bleibt.

## Teil 2: „Testen"-Knopf

Ein Knopf im Formular schickt mit den **gerade eingegebenen, noch nicht
gespeicherten** Daten einen minimalen Test-Request und zeigt das Ergebnis
inline. Er ist derselbe Knopf, der in Spec 0050, Teil 3 beschrieben ist;
dieses Formular hostet ihn nur.

- Drei unterscheidbare Ergebnisse: **gültig**, **Authentifizierung
  fehlgeschlagen**, **nicht erreichbar**.
- Ist beim generischen Typ keine Basis-URL ausgefüllt, geht ein Token
  **nie** an `api.openai.com`; derselbe Schutz gilt für die
  Modell-Entdeckung (Spec 0025).
- Der Key erscheint in keinem Log (Redaction).

## Teil 3: Gestaltung des Formulars

Das Anbieter-Formular und sein Optionsbereich sind klar gegliedert:

- Felder sind durch Rahmen oder Hintergrund abgegrenzt, der Fokuszustand
  ist sichtbar, Label und Feld sind eindeutig zugeordnet.
- Verwandte Felder (Anbietertyp, Key, Basis-URL, Modell) sind visuell
  gruppiert.
- Aktionen (Speichern, Testen, Löschen) sind als solche erkennbar und
  hierarchisch gestaltet (primär/sekundär).
- Die Gestaltung folgt derselben visuellen Sprache wie die zweispaltigen
  Einstellungen (Spec 0050).
- Format-Hinweis und Test-Ergebnis erscheinen inline im Formular, nicht
  als aufpoppende Fremd-Elemente.
- Es ändert sich nur die Darstellung, nicht die Anbieter-Logik.

## Akzeptanzfälle

- Falscher Präfix zeigt den Hinweis, Speichern bleibt möglich;
  generischer Anbieter und richtiger Präfix zeigen keinen Hinweis.
- Gültiger, falscher und unerreichbarer Key liefern je ihr eigenes
  Ergebnis.
- Ohne Basis-URL geht kein Token an `api.openai.com`.
