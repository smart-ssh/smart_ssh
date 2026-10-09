# Spec 0011 — Regel-Schnellvorschlag im Bestätigungsdialog

Status: umgesetzt
Zweck: Im Bestätigungsdialog führt ein Klick das Kommando aus und legt zugleich eine passende Allow-Regel an, damit ähnliche Kommandos künftig nicht erneut bestätigt werden müssen.
Bezüge: Spec 0002 (Filter-Engine), Spec 0009 (Regelverwaltung), Spec 0007 (Bestätigungsdialog), Spec 0077 (ungültige Muster), ADR 0002 (Abgleich mit und ohne `sudo`).

## 1. Ziel

Im Bestätigungsdialog für `Confirm`-Kommandos (weder Hard-Blacklist noch
Allow-Regel hat gegriffen) gibt es neben „Ausführen" und „Ablehnen" die
Schaltfläche **„Akzeptieren + Regel"** mit einem Aufklappmenü sinnvoller
Muster-Vorschläge.

## 2. Muster-Vorschläge

Die Heuristik ist bewusst einfach und erhebt keinen Anspruch auf
Vollständigkeit. Es gibt höchstens drei Vorschläge, doppelte werden entfernt:

- **Exakt:** das Kommando selbst, unverändert.
- **Basis-Wildcard:** erstes Wort plus ` *`, z. B. `ls -la /var/log` →
  `ls *`; nur, wenn das Kommando mehr als ein Wort hat. Ist das erste Wort ein
  `sudo`/`doas` oder ein durchreichender Wrapper wie `env`, wird stattdessen
  das zweite Wort verwendet (`apt *` statt `sudo *`): Ein Vorschlag `sudo *`
  würde jedes `sudo`-Kommando automatisch ausführbar machen. Dank des
  Abgleichs mit und ohne `sudo` (ADR 0002) deckt `apt *` auch `sudo apt …` ab.
- **Subkommando-Wildcard:** wenn das zweite Wort nicht mit `-` beginnt (also
  wie ein Subkommando aussieht, nicht wie eine Option), z. B.
  `systemctl status nginx` → `systemctl status *`.

Ein Vorschlag, dessen Muster sich nicht übersetzen lässt (z. B. wegen einer
einzelnen `[` im Kommando), wird weggelassen (Spec 0077).

## 3. Akzeptieren und Regel anlegen

Der Klick auf einen Vorschlag tut zwei Dinge:

1. Er legt eine Regel mit der Aktion **Allow** an. Die Aktion ist fest, weil
   eine Confirm-Regel gegenüber dem Standardfall nichts bringt. Die Priorität
   ist 0, sofern nicht anders angegeben. Die Anlage läuft über dieselbe Logik
   und Prüfung wie das Regel-Formular (Spec 0009, Spec 0077).
2. Er löst die wartende Bestätigung auf, exakt wie ein normales „Ausführen".

Die Bestätigung wird auch dann aufgelöst, wenn die Regel nicht angelegt
werden konnte (z. B. ungültiges Muster); der Fehler kommt getrennt zurück
und wird übersetzt angezeigt.

Die neue Regel wirkt nicht rückwirkend auf das gerade laufende Kommando. Es
läuft aufgrund der ausdrücklichen Bestätigung in diesem Moment, nicht wegen
der neuen Regel. Erst künftige, ähnliche Vorschläge profitieren.

## 4. Oberfläche

Das Aufklappmenü zeigt die Vorschläge mit Beschriftung und Muster-Vorschau.
Daneben wählt der Nutzer den Geltungsbereich: **„Dieser Server"** (Vorgabe,
die sicherere Wahl), „Global" oder „Tag" (mit Tag-Name). Die Auswahl entspricht
der im Regel-Formular (Spec 0009). Ein Klick auf einen Vorschlag schließt den
Dialog.

## 5. Grenzen

- Es wird nur die Aktion Allow angeboten.
- Die Vorschläge sind keine sicherheitsrelevante Auswertung: Sie zerlegen das
  Kommando nur nach Wörtern und erkennen weder Verkettung noch Substitution.
  Ob ein später passendes Kommando automatisch läuft, entscheidet allein die
  Filter-Engine (Spec 0002).
