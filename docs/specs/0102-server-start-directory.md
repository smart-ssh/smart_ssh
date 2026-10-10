# Spec 0102 — Startverzeichnis je Server für Terminal und Dateibrowser

Status: umgesetzt
Zweck: Ein Server-Profil kann ein optionales Startverzeichnis tragen. Ist es gesetzt, starten das interaktive Terminal und der SFTP-Dateibrowser einer Sitzung dort statt im Home des Login-Nutzers. Ist es leer, bleibt alles wie ohne dieses Feld. KI-Kommandos laufen weiterhin immer im Home.
Bezüge: Spec 0003 (Server-Profil), Spec 0005 (Terminal), Spec 0032 (lokaler Pseudo-Server), Spec 0067 (erhöhter Dateibrowser), Spec 0075 (SSH-Config-Import/-Export), ADR 0059 (KI-Kommandos in frischer Shell im Home), ADR 0101 (`cd`-Zeile im Terminal), ADR 0102 (eine Prüfung pro Sitzung).
Review-Priorität: NORMAL (Filter-Engine, Risiko-Klassifizierer, Redaction, Credentials und KI-Ausführungspfad bleiben unberührt)

## 1. Überblick

Das Startverzeichnis ist ein optionales Feld am Server-Profil (im Server-
Formular, nicht beim lokalen Pseudo-Server). Es gilt für das interaktive
Terminal und den Dateibrowser der Sitzung; beide nutzen dieselbe, einmalige
Prüfung (§4.1). Bestehende Server ohne Wert verhalten sich unverändert.

## 2. Datenmodell

- Der Wert ist optional; „nicht gesetzt" ist der Normalfall. Bestehende
  Server behalten nach einem Update „nicht gesetzt".
- Ein fehlendes Feld in einer Eingabe bedeutet „nicht gesetzt".

## 3. Validierung

Dieselbe Regel gilt im Server-Formular vor dem Speichern und im Backend
beim Anlegen und Bearbeiten (das Backend lehnt ab, bevor etwas gespeichert
wird):

1. Leerraum am Rand wird entfernt; leer = nicht gesetzt.
2. Erlaubt: ein absoluter Pfad (`/…`) oder ein Pfad, der mit `~/` beginnt.
   Abgelehnt: jeder andere relative Pfad, auch `~` allein und `~nutzer/…`.
   Das Formular zeigt eine klare Meldung und speichert nicht.
3. Steuerzeichen werden abgelehnt (sie würden die `cd`-Zeile im Terminal
   zerreißen oder vorzeitig abschicken, s. ADR 0101).

## 4. Verhalten in der Sitzung

### 4.1 Eine Prüfung pro Sitzung

Das konfigurierte Verzeichnis wird **einmal** pro Sitzung über den normalen
SFTP-Kanal der Sitzung geprüft. Terminal und Dateibrowser teilen dieses
Ergebnis (parallele Anfragen warten auf dieselbe Prüfung):

- **gefunden** — das Ziel existiert und ist ein Verzeichnis;
- **fehlt** — das Ziel existiert nicht, ist kein Verzeichnis, ist nicht
  zugänglich, oder der SFTP-Kanal lässt sich nicht öffnen;
- **nicht gesetzt** — keine Prüfung, kein SFTP-Zugriff.

Da SFTP kein `~` kennt, aber relative Pfade gegen das Home auflöst, wird
`~/x` als `./x` geprüft und geöffnet; `~/` allein ist das Home (`.`).

### 4.2 Terminal

Nach dem Start der Login-Shell schreibt die Anwendung sichtbar eine
`cd`-Zeile samt Eingabetaste ins Terminal, wenn das Verzeichnis gefunden
wurde: `cd -- '<Verzeichnis>'`. Bei `~/…` steht nur `~/` ungequotet (damit
die Shell die Tilde expandiert), der Rest ist gequotet; `~/` allein ergibt
`cd -- ~`. Ein `'` im Pfad wird maskiert. Die Shell-Anfrage selbst ist
unverändert, es gibt kein `cd … && exec $SHELL`. Scheitert das Schreiben der
Zeile, bleibt die Shell im Home und der Fehler wird nur protokolliert.
Quoting und Sonderfälle: ADR 0101.

### 4.3 Dateibrowser

Der Dateibrowser fragt beim Öffnen den Startpfad der Sitzung ab und öffnet
dort. „Zum Startverzeichnis" kehrt dorthin zurück, „Aufwärts" funktioniert
von dort ganz normal (`/srv/app` → `/srv`, `./projects` → `.`). Scheitert
schon die Abfrage, startet der Browser wie ohne Startverzeichnis bei `.`
(Home).

### 4.4 Fehlendes Verzeichnis

Terminal (kein `cd`) und Dateibrowser (`.`) nutzen das Home. Die Sitzung
scheitert nicht. Genau **ein** sichtbarer, nicht blockierender Hinweis pro
Sitzung nennt das konfigurierte Verzeichnis: Das Backend gibt den Wert nur
beim ersten Abruf heraus — an das Terminal oder den Dateibrowser, je
nachdem, wer zuerst fragt — und das Frontend zeigt ihn als Meldung
(Toast).

### 4.5 Nicht gesetzt

Kein SFTP-Zugriff, kein `cd`, Browser bei `.` — wie ohne dieses Feature.

## 5. Abgrenzungen und Grenzen

- **KI-Kommandos** (Chat und MCP) laufen weiter im Home (ADR 0059).
  Ausführungspfad, Filter-Engine und Risiko-Klassifizierer lesen den Wert
  nicht; das ausgeführte Kommando ist wörtlich der Vorschlag, mit und ohne
  Startverzeichnis.
- **Keine Sicherheitsgrenze:** Terminal und SFTP laufen ohnehin nicht durch
  die Filter-Engine; das Startverzeichnis ist Komfort.
- **Lokaler Pseudo-Server:** Das Formular bietet das Feld nicht an (wie die
  übrigen in Spec 0032 ausgeblendeten Felder), das synthetische Profil trägt
  immer „nicht gesetzt", das Verhalten bleibt unverändert.
- **Erhöhter Dateibrowser-Modus (Spec 0067):** Das Startverzeichnis wird für
  den Login-Nutzer geprüft; „Zum Startverzeichnis" lädt denselben Pfad auch
  im erhöhten Modus, eine eigene Prüfung für den Zielnutzer gibt es nicht.
  Ein Startverzeichnis der Form `~/…` wird dabei als relativer Pfad `./…`
  geladen (§4.1), den der erhöhte `sftp-server` aus Sicht des Zielnutzers
  auflöst, nicht gegen das Home des Login-Nutzers. „Zum Startverzeichnis"
  kann im erhöhten Modus deshalb in einem anderen Verzeichnis landen oder
  scheitern. Bekannte Grenze, kein Defekt; wer im erhöhten Modus verlässlich
  am selben Ort starten will, trägt einen absoluten Pfad ein.
- **Vorab-Eingabe im Terminal (Typeahead):** Die `cd`-Zeile (§4.2) wird nach
  dem Login ins Terminal geschrieben und wartet dort als Vorab-Eingabe, bis
  die Shell sie liest. Login-Skripte, die anstehende Eingaben verwerfen
  (z. B. per `tcflush` oder `read` in einer Schleife) oder per `exec` einen
  Multiplexer starten (z. B. `exec tmux`), können die Zeile verschlucken
  oder an den Multiplexer weiterreichen; das Terminal bleibt dann im Home
  bzw. das `cd` läuft in der falschen Umgebung. Hingenommen: Die Zeile ist
  sichtbar, und die Shell-Anfrage bleibt bewusst unverändert (ADR 0101).

## 6. SSH-Config-Import/-Export (Spec 0075)

Das Feld hat in `ssh_config` kein Gegenstück und wird nicht abgebildet.
Der Export benennt es nach Spec 0075, §3.2.3, als Kommentar über dem Block
(`# smart-ssh: Startverzeichnis (…) ist hier nicht abgebildet.`), auch bei
Zeilenumbrüchen im Wert ohne aus dem Kommentar auszubrechen; der Import
legt Server immer ohne Startverzeichnis an.

(Der frühere Abschnitt 7 „Tests" ist entfallen; die Nummer wird nicht neu
vergeben.)
