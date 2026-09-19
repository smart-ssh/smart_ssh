# Spec: Diagnose-Export in der App

Status: Entwurf
Repo: **öffentlich** `smart_ssh`, Frontend (Button/Ansicht) + Core (Paket
zusammenstellen, redigieren)
Abhängigkeiten: Startup-Logging (0047 B1), Version/Build-Hash (0052), Redactor
(0006 + Härtung), Datenpfade (0004)
Release-Gate: **I (MUSS)** — „spart am Launch-Tag Stunden Rückfragen".

> **Zweck:** Ein Nutzer/Tester klickt **einen Knopf**, und die App stellt ein
> **redigiertes** Diagnosepaket zusammen (Version+Hash, OS, Datenpfade, letzte
> Log-Zeilen), das er an den Support/ins Issue hängen kann. Ohne das beginnt
> bei jedem „geht nicht" das mühsame Erfragen von Version/OS/Log. **Priorität
> NORMAL**, aber mit einer **sicherheitsrelevanten** Anforderung: das Paket
> darf keine unredigierten Secrets enthalten (es wird oft in **öffentliche**
> GitHub-Issues gehängt).

## 1. Zwei Funktionen (Release-Gate I)

- **Logpfad öffnen**: Ein Knopf, der den Ordner mit den Logdateien im
  OS-Dateimanager öffnet (Finder/Explorer/…). Für Nutzer, die selbst
  reinschauen wollen. Trivial, aber nützlich.
- **Diagnosepaket erzeugen**: Ein Knopf, der ein **redigiertes** Paket
  zusammenstellt (siehe §2) und dem Nutzer zum Speichern/Ansehen gibt.

Beide erreichbar an einem sinnvollen Ort (Einstellungen → „Über"/„Diagnose",
passt zur zweispaltigen Settings-Struktur 0050, wo auch Version+Hash schon
sichtbar sind).

## 2. Was ins Diagnosepaket kommt

- **Version + Build-Hash** (aus 0052 — der private Hash, der das Binary
  eindeutig identifiziert).
- **OS/Plattform** (Betriebssystem, Version, Architektur).
- **Datenpfade** (wo DB/Logs/Config liegen — hilft bei Pfad-/Rechteproblemen).
- **Die letzten N Log-Zeilen** (z. B. die letzten 500 oder die letzte
  Logdatei) — **redigiert** (§3).
- **App-Zustand, soweit unkritisch**: aktive Edition (Community/Official),
  konfigurierte Provider-**Typen** (nicht Keys!), Anzahl Server (nicht deren
  Details). Nur was bei der Fehlersuche hilft, nichts Sensibles.
- **KEINE**: API-Keys, Passwörter, Host-Keys, Server-Adressen/Zugangsdaten,
  Notiz-Inhalte, Chat-Inhalte. Das Paket beschreibt den *Zustand der App*,
  nicht die *Daten des Nutzers*.

## 3. Redaction — PFLICHT (die sicherheitsrelevante Anforderung)

- Die eingepackten Log-Zeilen laufen durch den **Redactor** (0006 + Härtung:
  Shadow/Crypt-Hashes, DB-Strings, Provider-Tokens, die generischen Muster),
  **bevor** sie ins Paket kommen. Auch wenn die Logs *eigentlich* schon
  redigiert sind (0016) — hier **zusätzlich** drüber, als Sicherheitsnetz
  (Defense in Depth), weil das Paket oft öffentlich geteilt wird.
- **Der Nutzer kann das Paket VOR dem Senden ansehen** (Vorschau/es wird als
  Datei gespeichert, die er selbst öffnen kann) — Transparenz: „das wird
  geteilt, schau selbst drüber". Kein automatisches Hochladen/Versenden.
- Falls trotz Redaction etwas Sensibles im Log stünde, das der Redactor nicht
  kennt (die bekannte musterbasierte Grenze), ist die Nutzer-Vorschau die
  zweite Verteidigungslinie.

## 4. Format

Ein einzelnes, gut lesbares Artefakt — Vorschlag: eine **`.txt`- oder
`.md`-Datei** (leicht in ein Issue zu pasten) oder ein **`.zip`**, falls
mehrere Logdateien mitmüssen. Klar strukturiert (Abschnitte Version/OS/Pfade/
Logs), damit du als Support sofort das Relevante siehst. Wähle das Sinnvollste
und beschreibe es mir.

## Invarianten / Sicherheit
- **Keine Keys/Passwörter/Zugangsdaten/Nutzerinhalte** im Paket (nur
  App-Zustand + redigierte Logs).
- Log-Zeilen laufen **zusätzlich** durch den Redactor (Defense in Depth).
- **Kein automatisches Versenden** — der Nutzer sieht/speichert das Paket
  selbst und entscheidet, es zu teilen.
- Das Erzeugen des Pakets ändert nichts an den Daten (rein lesend).

## Testbarkeit
- Paket enthält Version+Hash, OS, Pfade, redigierte Logs — und **keine** der
  ausgeschlossenen Kategorien (Test: ein Fake-Secret in einer Logzeile taucht
  im Paket **redigiert** auf; ein konfigurierter (Fake-)API-Key taucht **gar
  nicht** auf).
- Logpfad-öffnen ruft den richtigen OS-Pfad auf.
- Rein lesend (keine Datenänderung).

## Reihenfolge
1. Logpfad-öffnen (trivial, sofort nützlich).
2. Diagnosepaket-Sammlung (Version/OS/Pfade/Zustand).
3. Log-Einbindung + **Redaction** + Nutzer-Vorschau.

## Abschluss
- `spec-reviewer` ERHÖHT (weil Redaction/Secret-Ausschluss im Spiel — ein
  Diagnose-Export, der Keys leakt, wäre ein ernster Fund).
- CHANGELOG.
- Melde mir: das gewählte Format, was genau ins Paket kommt (die finale
  Feldliste), und dass ein Fake-Secret im Log redigiert + ein Fake-Key gar
  nicht im Paket landet.
- **Nicht visuell testbar** — beschreibe mir einen manuellen Testablauf für
  Stefan (Paket erzeugen, reinschauen, prüfen dass nichts Sensibles drin ist).
