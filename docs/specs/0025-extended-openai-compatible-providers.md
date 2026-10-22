# Spec 0025 — Erweiterte OpenAI-kompatible Anbieter

Status: umgesetzt
Zweck: Anbieter, die der OpenAI-API folgen (z. B. OpenRouter, TEE-gehostete
Dienste), bequem nutzbar machen: Modellauswahl, Zusatz-Header und eine rein
informative Attestierungs-Anzeige.
Bezüge: Spec 0006 (Anbieter-Abstraktion), Spec 0007 (Anbieter-Verwaltung).

## 1. Ziel

Der generische OpenAI-kompatible Anbietertyp deckt technisch bereits
OpenRouter und die meisten anderen kompatiblen Anbieter ab. Diese Spec
beschreibt die Nutzbarkeit dafür: Modellauswahl statt Freitext,
anbieterspezifische Zusatz-Header und eine **informative, nicht
kryptografisch geprüfte** Anzeige für TEE-gehostete Anbieter.

## 2. Modell-Entdeckung

Die App fragt die Modellliste des Anbieters ab (`GET {Basis-URL}/models`,
OpenAI-Konvention) und bietet die IDs im Formular als durchsuchbares
Dropdown an — für den generischen OpenAI-kompatiblen Typ, OpenAI und
Ollama. Schlägt die Abfrage fehl (nicht jeder Anbieter unterstützt den
Endpunkt), fällt das Feld auf Freitext zurück; das Anlegen eines Anbieters
wird dadurch nie blockiert.

### Auslöser

Die Modellsuche läuft nur auf eine Nutzeraktion hin:

- **Verlassen des API-Key-Felds** (oder des Base-URL-Felds), wenn ein Key
  eingegeben ist, der Anbietertyp die Suche unterstützt und eine nötige
  Base-URL gesetzt ist. Bloßes Tippen löst nie eine Anfrage aus. Ein
  erneutes Verlassen des Felds ohne Änderung an Typ, Base-URL oder Key löst
  keine weitere Anfrage aus; das gilt auch, wenn die letzte Suche per Knopf
  lief. Ein leeres Key-Feld löst nichts aus.
- **Klick auf „Modelle laden"**, jederzeit als manueller neuer Versuch.

Für Ollama gibt es keinen automatischen Auslöser über das Key-Feld; dort
deckt die eigene Ollama-Suche das ab (Spec 0069).

Je Formular läuft höchstens eine Suche gleichzeitig. Ändern sich Typ,
Base-URL oder Key, während sie läuft, wird ihr Ergebnis verworfen; die
nächste Suche startet beim nächsten Verlassen des Felds bzw. per Knopf.

### Ergebnisse

- **Modelle gefunden:** Sie erscheinen als Vorschläge im Modellfeld.
- **Leere Liste:** neutraler Hinweis „Der Anbieter hat keine Modelle
  geliefert. Modellname manuell eingeben." Das Feld bleibt nutzbar.
- **Zugangsdaten abgelehnt** (der Anbieter weist den Key zurück): ein
  eigener, roter Hinweis direkt am Key-Feld, im selben Stil wie das Ergebnis
  von „Zugangsdaten testen" — nicht der allgemeine Hinweis. Er verschwindet,
  sobald Key, Base-URL oder Typ geändert werden.
- **Jeder andere Fehler** (Netzwerk, Zeitüberschreitung, nicht erreichbar,
  Endpunkt nicht unterstützt): Das Modellfeld bleibt Freitext, mit dem
  bisherigen, nicht blockierenden Hinweis.

In keinem Fall blockiert die Suche das Speichern des Anbieters. Der Key geht
dabei nur an den Anbieter (wie bei „Modelle laden"), er wird weder
protokolliert noch gespeichert, bevor der Nutzer den Anbieter anlegt.

## 3. Zusatz-Header

Eine Anbieter-Konfiguration kann beliebige zusätzliche HTTP-Header tragen
(generisch, nicht OpenRouter-spezifisch; OpenRouter nutzt z. B. optional
`HTTP-Referer`/`X-Title`). Sie werden an jede Anfrage an diesen Anbieter
angehängt. Im Formular stehen sie als einfache Schlüssel-Wert-Liste hinter
„Erweitert".

## 4. TEE-gehostete Anbieter — Informationsanzeige

Es gibt **keine** kryptografische Verifikation: Sie wäre hardware- und
anbieterspezifisch und würde falsche Sicherheit suggerieren.

Stattdessen kann die Konfiguration (unter „Erweitert") eine optionale
Attestierungs-URL tragen. Ist sie gesetzt, ruft die App sie beim Speichern
und auf Wunsch erneut ab und zeigt die **rohe Antwort** in einem
schreibgeschützten Textblock, mit dem Hinweis:

> „Dieser Wert wird unverändert vom Anbieter abgerufen und **nicht** von
> Smart SSH kryptografisch geprüft. Zur eigenständigen Verifikation nutze
> das vom Hardware-/Anbieter bereitgestellte Prüfwerkzeug."

Es gibt kein Badge, kein grünes Häkchen und kein Wort wie „verifiziert":
Die App zeigt nur an, was der Anbieter selbst meldet.

## 5. Grenzen

- Eine echte Attestierungsprüfung ist nicht Teil des Produkts.
- Die entdeckten Modelle zeigen keine Zusatzdaten wie Preis oder
  Kontextfenstergröße; der generische OpenAI-Standard liefert sie nicht.

## 5. Anbieter hinzufügen im Dialog

Auf der Einstellungsseite „KI-Provider" steht unter der Liste der
konfigurierten Anbieter die Schaltfläche „Provider hinzufügen" — sowohl bei
leerer als auch bei gefüllter Liste (bei leerer Liste als hervorgehobene
Hauptaktion). Das Anlegeformular ist nicht mehr Teil der Seite, sondern
öffnet sich in einem Dialog über den Einstellungen; er enthält alle Felder
des Formulars unverändert (Typ, Name, Modell mit Modellsuche, Base-URL,
API-Key mit Zugangsdaten-Test, „Erweitert", Hinweis „wird als aktiver Provider
verwendet").

- „Hinzufügen" speichert den Anbieter, schließt den Dialog und zeigt den
  neuen Anbieter in der Liste; war noch keiner aktiv, ist er aktiv.
- Schlägt das Speichern fehl, bleibt der Dialog mit allen Eingaben offen
  und der Fehler steht im Dialog. Gelingt das Speichern, aber nicht das
  Aktivieren, schließt der Dialog, der Anbieter steht in der Liste und der
  Fehler erscheint auf der Seite.
- „Abbrechen" und Escape schließen den Dialog und verwerfen alle Eingaben
  (auch den API-Key); Escape schließt nur den Dialog, nicht die
  Einstellungen. Ein Klick außerhalb schließt ihn nicht. Beim erneuten
  Öffnen ist das Formular leer, Modellsuche und Test sind zurückgesetzt.
- Der Fokus wandert beim Öffnen in den Dialog, bleibt darin und kehrt beim
  Schließen zur Schaltfläche „Provider hinzufügen" zurück. Im kleinen
  Fenster bleiben Titel und Aktionen erreichbar; nur der Inhalt scrollt.
