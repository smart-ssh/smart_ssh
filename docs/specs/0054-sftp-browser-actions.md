# Spec: SSH-/SFTP-Dateibrowser — Aktionen & Menüs

Status: Entwurf
Repo: **öffentlich** `smart_ssh` (Kern + Frontend); ein Teil (lokal öffnen,
Standardprogramme) berührt Tauri-Shell/OS-Integration
Modul: SFTP-Dateibrowser (0020), Kontextmenü, Drei-Punkte-Menü
Abhängigkeiten: SFTP-Browser (0020), Datei-Write-Diff (0020), lokaler
Pfad-Handling, Tauri-Shell-Plugin (für „lokal öffnen")

> Baut den manuellen Dateibrowser zu einem vollwertigen Dateimanager aus:
> Kontextmenü + Drei-Punkte-Menü mit allen üblichen Aktionen, plus ein
> „lokal bearbeiten → Upload anbieten"-Flow.

## Sicherheitsmodell — WICHTIG, zuerst lesen

**Manuelle Dateibrowser-Aktionen laufen NICHT durch die Filter-Engine und
NICHT durch den KI-Zweitmeinungs-Check.** Begründung: Die Filter-Engine
schützt vor der **KI** (weniger vertrauenswürdiger Akteur, deren Vorschläge
geprüft werden müssen). Wenn der **Nutzer selbst** im Browser eine Aktion
auslöst, ist er der vertrauenswürdige Akteur — genau wie beim manuellen
Tippen im Terminal. Ihn durch die Filter-Engine zu zwingen, wäre ein
Missverständnis des Prinzips und würde das Werkzeug unbenutzbar machen.

**Stattdessen gelten für manuelle Aktionen:**
1. **Normale Bestätigung für gefährliche/irreversible Aktionen** (gute UX,
   kein Sicherheits-Gate): Löschen, Überschreiben. Kein Dialog für harmlose
   (Herunterladen, Kopieren, Umbenennen).
2. **Audit-Erfassbarkeit**: Alle **server-verändernden** Aktionen (chmod,
   Löschen, Umbenennen, Verschieben, Ordner anlegen, Upload) müssen so
   gebaut sein, dass sie später in den zentralen Audit-Log fließen können
   (siehe Backlog „Zentrale Audit-Schicht"). Quelle „manuell" (nicht „KI").
   Der Audit-Log selbst ist ein späteres Feature — aber die Aktionen jetzt
   nicht so bauen, dass sie ihn umgehen.
3. **Bestehende Schutzmechanismen greifen weiter**: Der Datei-Write-Diff-
   Dialog (0020) beim Überschreiben, die Sudo-Fallback-Anzeige bei
   privilegierten Writes, Pfad-Validierung.

## Teil 0: Menü-Bug fixen (Voraussetzung)

Das **Drei-Punkte-Menü** hinten an den Einträgen funktioniert aktuell
**nicht**. Das ist die Grundlage für alles Weitere — zuerst reparieren.
Beschreibe mir die Ursache. Danach trägt es (und das Kontextmenü) die neuen
Aktionen.

## Teil 1: Kontextmenü + Drei-Punkte-Menü aufbauen

Beide Wege zeigen **dieselben** Aktionen (Rechtsklick = Kontextmenü;
Drei-Punkte hinten = dasselbe Menü). Aktionen abhängig vom Typ (Datei vs.
Ordner) und vom Kontext (eine vs. mehrere Auswahl, wo sinnvoll).

## Teil 2: Lesende / lokale Aktionen (unkritisch, keine Bestätigung)

- **Herunterladen** — in ein konfigurierbares **Standard-Downloadverzeichnis**
  ODER per Dialog an einen **präzisen Pfad**. Ordner rekursiv. Fortschritt bei
  großen Dateien.
- **Dateiinhalt kopieren** — den Inhalt (Text) in die Zwischenablage. Bei
  Binärdateien deaktiviert/Hinweis.
- **Pfad kopieren** — den Remote-Pfad in die Zwischenablage (praktisch für
  Kommandos).
- **Eigenschaften/Info anzeigen** — Detailansicht: Größe, Rechte (numerisch +
  symbolisch), Besitzer/Gruppe, Änderungsdatum, Typ. (Gehört thematisch zur
  Rechte-Bearbeitung, Teil 3.)
- **Aktualisieren** — den aktuellen Ordner neu laden.
- **Lokal öffnen** (siehe Teil 4 — Flow mit Upload-Angebot).

## Teil 3: Server-verändernde Aktionen (normale Bestätigung + audit-erfassbar)

Kein Filter-Engine-/KI-Gate — aber Bestätigung bei irreversibel, und so
gebaut, dass der Audit-Log sie später erfassen kann.

- **Rechte bearbeiten (chmod)** — UI zum Setzen der Permissions
  (Checkbox-Matrix owner/group/other × read/write/execute, plus numerische
  Eingabe). Optional rekursiv für Ordner (mit deutlicher Kennzeichnung, weil
  mächtig). Server-verändernd → audit-erfassbar.
- **Löschen** — Datei/Ordner. **Bestätigung Pflicht** (irreversibel), bei
  Ordnern mit Hinweis auf rekursives Löschen und Anzahl. Analog zum
  zweistufigen `delete_server` (Vorschau, was gelöscht wird).
- **Umbenennen** — Inline-Edit oder Dialog. Kollisionsprüfung (Zielname
  existiert schon → Warnung).
- **Neuen Ordner anlegen** — im aktuellen Verzeichnis.
- **Verschieben** — innerhalb des Servers (Cut/Paste oder Drag-and-Drop
  zwischen Ordnern). Überschreib-Warnung bei Zielkollision.
- **Hochladen** — lokale Datei(en) in den aktuellen Remote-Ordner (per Dialog
  oder Drag-and-Drop). Überschreibt bestehende → **Diff-Vorschau (0020)**.

## Teil 4: „Lokal öffnen → bearbeiten → Upload anbieten"-Flow

Der anspruchsvollste Teil (wie „Edit with…" in Cyberduck/Transmit):

1. **Herunterladen** in einen kontrollierten temporären lokalen Pfad (nicht
   irgendwo — ein definiertes Temp-Verzeichnis der App).
2. **Mit lokalem Programm öffnen** — entweder das OS-Standardprogramm für den
   Dateityp, oder ein vom Nutzer **festgelegtes Standardprogramm** (siehe
   Teil 5).
3. **Lokale Datei überwachen** (Datei-Watcher), solange sie „in Bearbeitung"
   ist.
4. **Bei Änderung** (Nutzer speichert im lokalen Programm) → erkennen →
   Benachrichtigung „Datei X wurde lokal geändert. Auf den Server
   hochladen?".
5. **Bei Ja** → hochladen (server-verändernd, audit-erfassbar), **mit
   Diff-Vorschau** (0020) und **Konflikt-Prüfung**: Hat sich die Remote-Datei
   seit dem Download geändert (z. B. Änderungsdatum/Hash)? → Warnung vor dem
   Überschreiben (Datenverlust vermeiden).
6. **Sauberes Ende**: Watcher stoppt, wenn der Nutzer den Flow beendet;
   Temp-Datei aufräumen (bei Session-Ende spätestens).

Fehlerfälle: lokales Programm nicht gefunden, Datei schon offen, Upload
scheitert — alle sichtbar behandeln.

## Teil 5: Standardprogramme festlegen (rein lokal)

- Einstellung: **pro Dateityp/Endung ein Standardprogramm** festlegen (z. B.
  `.conf` → VS Code, `.log` → der Standard-Editor). Fallback: OS-Standard.
- Verwaltung in den Einstellungen (passt zur neuen zweispaltigen Settings-
  Struktur, „Dateien"- oder „Editor"-Sektion).
- Rein lokal, kein Server-Bezug, keine Sicherheitsfrage.

## Invarianten / Sicherheit

- Manuelle Aktionen: **kein** Filter-Engine-/KI-Check (Nutzer ist
  vertrauenswürdiger Akteur).
- Irreversible Aktionen (Löschen, Überschreiben): **Bestätigung Pflicht**.
- Server-verändernde Aktionen: **audit-erfassbar** gebaut (Quelle „manuell"),
  auch wenn der Audit-Log selbst noch nicht existiert — nicht so bauen, dass
  er umgangen würde.
- Überschreiben/Upload: **Diff-Vorschau (0020)** + Konflikt-Prüfung
  (Remote-Änderung seit Download).
- Temp-Dateien des Lokal-Öffnen-Flows in einem **kontrollierten** Pfad,
  aufgeräumt.
- Frontend-Pfade vom Nutzer werden validiert (bestehende Pfad-Validierung).

## Testbarkeit

- Menü-Bug: Drei-Punkte + Kontextmenü zeigen dieselben Aktionen, öffnen
  zuverlässig.
- Jede lesende Aktion: Download (Datei/Ordner/präziser Pfad), Inhalt kopieren,
  Pfad kopieren, Eigenschaften, Aktualisieren.
- Server-verändernd: chmod setzt Rechte; Löschen fragt + löscht; Umbenennen
  mit Kollision; Ordner anlegen; Verschieben mit Zielkollision; Upload mit
  Diff.
- Lokal-Öffnen-Flow: Download → öffnen → lokale Änderung erkannt → Upload
  angeboten → Diff + Konflikt-Prüfung → hochgeladen; Watcher sauber beendet;
  Temp aufgeräumt.
- Standardprogramme: pro Typ gesetzt, greift beim Öffnen, Fallback auf
  OS-Standard.

## Reihenfolge

1. **Teil 0** — Menü-Bug fixen (Grundlage).
2. **Teil 1 + 2** — Menüs + lesende/lokale Aktionen (unkritisch).
3. **Teil 3** — server-verändernde Aktionen (Bestätigung, audit-erfassbar).
4. **Teil 5** — Standardprogramme (rein lokal, Voraussetzung für Teil 4).
5. **Teil 4** — Lokal-Öffnen-Upload-Flow (komplexeste, baut auf 2+3+5 auf).

Große Spec — der Coder kann in dieser Reihenfolge in mehreren Commits/
Schritten liefern; sag mir, ob du sie als ein großes Paket oder etappenweise
umgesetzt haben willst.
