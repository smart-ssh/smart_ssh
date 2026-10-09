# Spec 0002 — Filter- und Policy-Engine

Status: umgesetzt
Zweck: Jedes Kommando, das die KI vorschlägt oder das aus einem KI-Ablauf entsteht, wird geprüft, bevor es an eine SSH-Sitzung geht. Die Engine entscheidet, ob es automatisch laufen darf, bestätigt werden muss oder gar nicht erst angeboten wird.
Bezüge: Spec 0009 (Regelverwaltung, erklärte Auswertung), Spec 0037 (Herkunft von Regeln), Spec 0060 (Glob und `/`), Spec 0077 (ungültige Muster), Spec 0026 (Risiko-Hinweise), Spec 0092 (Rot verlangt Bestätigung), ADR 0001, ADR 0002.

## 1. Ziel

Die Engine liefert für ein Kommando genau eine von drei Entscheidungen:

- **AutoExec** — das Kommando läuft ohne Rückfrage.
- **Confirm** — der Nutzer muss bestätigen. Die Entscheidung trägt einen Grund.
- **Deny** — das Kommando wird gar nicht erst zur Bestätigung angeboten,
  etwa weil eine Deny-Regel greift oder es nicht sinnvoll prüfbar ist.

Kernprinzip: **Fail-safe.** Alles, was nicht eindeutig durch eine Allow-Regel
erlaubt ist, landet mindestens bei `Confirm`. Nichts wird automatisch
ausgeführt, außer es trifft ausdrücklich eine Allow-Regel. Ohne ladbare
Regeln gilt für jedes Kommando `Confirm`.

## 2. Regeln

Eine Nutzerregel besteht aus einem Muster, einer Aktion, einem Geltungsbereich
und einer Priorität.

- **Muster:** *Glob* (z. B. `ls *`, `cat /var/log/*`), *Regex* oder *Exakt*
  (das ganze Kommando, unverändert).
- **Aktion:**
  - *Allow* macht ein Kommando automatisch ausführbar.
  - *Confirm* erzwingt die Bestätigung, auch wenn eine Allow-Regel passt.
  - *Deny* blockiert das Kommando vollständig.
- **Geltungsbereich:** *Global* (gilt immer), *Server* (ein bestimmter Server)
  oder *Tag* (alle Server mit diesem Tag, z. B. `production`).
- **Priorität:** eine Zahl; höher wird zuerst geprüft.

Jede Regel hat zusätzlich eine Herkunft (eingebaut, Organisation, Nutzer;
Spec 0037).

## 3. Rangfolge

Die Reihenfolge der Auswertung ist nicht verhandelbar:

### 3.1 Hard-Blacklist

Eine fest eingebaute Liste gefährlicher Kommandos, die der Nutzer nicht
entfernen kann, etwa `rm -rf /`, `dd if=* of=/dev/*`, `mkfs*`, die
Fork-Bomb `:(){ :|:& };:`, direkte Änderungen an `/etc/shadow` sowie
`shutdown` und `reboot`. Ein Treffer ergibt immer mindestens `Confirm`, nie
`AutoExec`, unabhängig von Nutzerregeln.

### 3.2 Nutzerregeln

Nach der Hard-Blacklist werden die Nutzerregeln in dieser Reihenfolge
geprüft; der erste Treffer entscheidet:

1. **Deny-Regeln**
2. **Confirm-Regeln**
3. **Allow-Regeln**

Innerhalb einer Aktion gilt: zuerst nach Herkunft (Spec 0037, Abschnitt 5),
dann nach Spezifität des Geltungsbereichs (Server vor Tag vor Global), dann
nach Priorität.

Deny schlägt also immer Confirm, und Confirm schlägt immer Allow. Eine
Allow-Regel für `systemctl *` wird von einer Deny-Regel für
`systemctl stop nginx` auf Server-Ebene übersteuert. Stehen zwei
widersprüchliche Regeln gleicher Priorität und gleichen Geltungsbereichs
nebeneinander (Allow gegen Confirm), gewinnt die strengere.

### 3.3 Standard

Trifft nichts, lautet die Entscheidung `Confirm` mit dem Grund „keine Regel
gefunden".

## 4. Zerlegung und Schutz vor Verkettung

Dies ist der sicherheitskritischste Teil. Ein Vorschlag wie

```
ls -la && rm -rf /var/backup
```

wird nicht als Ganzes gegen `ls *` geprüft. Stattdessen gilt:

1. Das Kommando wird in Teilkommandos zerlegt, getrennt durch `&&`, `||`,
   `;`, `|` sowie durch Command-Substitution `$(...)` und Backticks.

   Schreibende Ausgabe-Umleitungen (`>`, `>>`, `2>`, `&>`) werden
   ebenfalls modelliert. Ein Umleitungsziel ist ein Schreibzugriff auf eine
   Datei und wird als solcher geprüft: `ls -la > /etc/passwd` läuft nicht
   automatisch unter einer Regel `Allow: ls *`, nur weil es mit `ls`
   beginnt. Das Ziel ist ein eigener Prüfbestandteil mit mindestens
   `Confirm`, kein bloßes Argument von `ls`.

   Eingabe-Umleitung (`<`) wird bewusst nicht gesondert behandelt. Ein
   Lesezugriff wie `cat < /etc/shadow` bekommt dieselbe Entscheidung wie
   `cat /etc/shadow`, weil das Kommando selbst schon auf die Datei zugreift.
2. Jedes Teilkommando durchläuft einzeln die vollständige Rangfolge aus
   Abschnitt 3.
3. Die Gesamtentscheidung ist das **strengste** Ergebnis aller Teile:
   `Deny` vor `Confirm` vor `AutoExec`. Ein einziges `Deny`-Teilkommando
   macht den ganzen Befehl zu `Deny`.
4. Kann das Kommando nicht sicher zerlegt werden (verschachtelte Quotes,
   ungewöhnliche Escape-Folgen, Here-Docs), lautet die Entscheidung
   `Confirm` mit dem Grund „Kommando konnte nicht sicher analysiert
   werden", nie `AutoExec`.
5. Command-Substitution (`$(...)`, Backticks) ist ein eigener Prüfbestandteil
   und erzwingt mindestens `Confirm`, weil sie zur Laufzeit anderen Code
   ausführen kann, der bei der Prüfung nicht vollständig bekannt ist.

   **Hinterlegter Code** wird genauso behandelt: Ein Kommando, das Code für
   später speichert, erzwingt mindestens `Confirm` mit einem Grund, der
   `trap` bzw. `alias` nennt, auch hinter `command`/`builtin`. Das betrifft
   einen `trap`-Handler (läuft bei einem Signal oder beim Beenden der
   Shell) und eine Alias-Definition `alias NAME=WERT` (läuft bei jedem
   späteren Aufruf des Namens). Lässt sich der hinterlegte Code als
   Zeichenkette lesen, wird er zusätzlich wie ein eigenes Kommando geprüft;
   eine Deny-Regel oder die Hard-Blacklist greift also auch darin. Reine
   Abfrage- und Rücksetzformen speichern keinen Code und bleiben
   unverändert: `trap`, `trap -p`, `trap -l`, `trap - SIG`, `trap '' SIG`,
   `alias`, `alias NAME`, `alias -p`.
6. **Wrapper werden vollständig abgeschält.** Ein `sudo`- oder
   `doas`-Präfix wird vor dem Abgleich entfernt und zusätzlich vermerkt
   („erhöhte Rechte"), sodass sich z. B. eine Regel „alles mit sudo →
   immer Confirm" formulieren lässt.

   Die Normalisierung bleibt nicht bei einem einzelnen Token stehen. Ein
   Angreifer kann beliebig umhüllen: `env rm -rf /`, `sudo -u root rm -rf /`,
   `sudo sudo rm -rf /`, `timeout 5 rm -rf /`, `xargs rm`, `chroot`, bloße
   `VAR=wert`-Präfixe oder `sudo bash -c "rm -rf /"`. Deshalb werden
   bekannte Wrapper (Rechteerhöhung, `env`, `timeout`, `xargs`, `chroot`,
   Variablenzuweisungen) so lange abgeschält, bis sich der Kommandokopf
   nicht mehr ändert; erst dann wird verglichen. Das normalisierte Kommando
   ist die Grundlage **jeder** Prüfung (Hard-Blacklist, Nutzerregeln,
   Risiko-Einstufung), damit keine Ebene eine schwächere Sicht hat als die
   anderen.

   `bash -c "..."` und `sh -c "..."` sind ebenfalls Wrapper: Der Inhalt des
   `-c`-Arguments wird als eigenes Kommando geprüft (bei Nicht-Parsebarkeit
   gilt `Confirm`), nicht als undurchsichtiges Argument durchgelassen.

   Dasselbe gilt für Code, den eine Shell aus einem Here-String liest
   (`bash <<< "rm -rf /"`, auch hinter `sudo` oder Wrappern): Er wird als
   eigenes Kommando geprüft, das Gesamtergebnis ist mindestens `Confirm` und
   wird `Deny`, wenn der Inhalt `Deny` ergibt. Lässt sich der Inhalt nicht
   eindeutig bestimmen (Here-Docs, mehrere Here-Strings, weitere
   Eingabe-Umleitungen, ein Here-String-Wort mit `$`- oder
   Backtick-Expansion, ein Ziel, das keine von stdin lesende Shell ist),
   bleibt es bei `Confirm`.
7. **Begrenzungen als Absturzschutz.** Vor dem Zerlegen wird die Länge
   geprüft (Standardgrenze 4096 Bytes). Gemessen wird in Bytes der
   UTF-8-Kodierung, nicht in Zeichen: Ein Kommando mit Mehrbyte-Zeichen
   (z. B. „€", 3 Bytes) erreicht das Limit mit weniger Zeichen. Alle
   Konsumenten messen gleich, ein Kommando liegt also entweder für alle
   innerhalb des Limits oder für alle darüber. Auch die Verschachtelungstiefe
   von Command-Substitution (`$(echo $(echo ...))`) ist begrenzt.
   Überschreitung ergibt `Confirm` mit entsprechendem Grund, nie einen
   unbegrenzten Abstieg, der den Prozess per Stack-Überlauf beenden könnte.
   Die Grenzen gelten für **jeden** Konsumenten der Zerlegung gleichermaßen,
   für die Filter-Engine wie für die Risiko-Einstufung.

## 5. Auswertungskontext und Auswertung

Eine Auswertung bekommt das Kommando und einen Kontext: den Server, auf dem
es laufen soll, und dessen Tags. Aus dem Kontext ergibt sich, welche Regeln
gelten (Global immer, Server und Tag nur bei Übereinstimmung).

Die Regeln kommen aus einer austauschbaren Quelle. Tests und die laufende
App nutzen dieselbe Auswertung, nur mit anderer Quelle; die Logik hängt
weder von SSH noch von einer KI-API noch von der Oberfläche ab.

Zusätzlich zur Entscheidung gibt es eine erklärte Auswertung, die
nachvollziehbar macht, welche Regel oder welcher Hard-Blacklist-Eintrag
gegriffen hat (Spec 0009, Abschnitt 4).

## 6. Prüffälle

| # | Eingabe | Erwartung | Grund |
|---|---------|-----------|-------|
| 1 | `ls -la` (Allow: `ls *`) | AutoExec | einfacher Allow-Treffer |
| 2 | `rm -rf /` | mindestens Confirm | Hard-Blacklist greift immer |
| 3 | `ls -la && rm -rf /var/backup` | strengstes Teilergebnis (Deny oder Confirm) | Verkettung umgeht die Blacklist nicht |
| 4 | `ls $(cat /etc/passwd)` | Confirm | Command-Substitution erzwingt Confirm |
| 5 | `systemctl status nginx` (Tag „production": Deny für `systemctl *`) | Deny | Server und Tag stehen vor Global |
| 6 | `echo "ls -la"` | wie für `echo *` konfiguriert | der Inhalt von `echo` wird nicht interpretiert |
| 7 | `sudo apt update` | Confirm, wenn die Regel „sudo → Confirm" aktiv ist | Kennzeichen „erhöhte Rechte" |
| 8 | Kommando mit verschachtelten oder unklaren Quotes | Confirm | nie AutoExec bei Zweifel |
| 9 | `ls -la; rm important.txt` (nur `ls` erlaubt) | Confirm | Standard für den unbekannten Teil |
| 10 | leere oder nur aus Leerraum bestehende Eingabe | Deny | kein sinnvolles Kommando |
| 11 | Kommando über dem Längenlimit (in Bytes) | Confirm | Schutz vor Verschleierung durch sehr lange Eingaben |
| 12 | zwei widersprüchliche Regeln gleicher Priorität (Allow gegen Confirm, gleicher Geltungsbereich) | Confirm | im Zweifel die strengere Regel |

## 7. Sicherheitszusagen

- Fail-safe: Zweifel führt zu `Confirm`, nie zu `AutoExec`.
- Eine Deny-Regel oder die Hard-Blacklist wird durch keinen weiteren Pfad
  abgeschwächt; Eskalation geht nur in eine Richtung (strenger).
- Das Kommando wird vor jeder Prüfung normalisiert, und alle Ebenen sehen
  dieselbe Form.
- Begrenzungen für Länge und Verschachtelung gelten für alle Konsumenten.

## 8. Grenzen

- Mehrzeilige Skripte und Here-Docs werden nicht im Einzelnen analysiert,
  sondern als Ganzes bestätigt (ADR 0001).
- Die Simulationsansicht („teste diese Regel gegen ein Beispielkommando")
  ist in Spec 0009 beschrieben.
- Zeitlich befristete Regeln gibt es nicht.
