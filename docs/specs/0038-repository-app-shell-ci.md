# Spec 0038 — Repository-Trennung, Anwendungs-Hülle und Erweiterungspunkte

Status: umgesetzt
Zweck: Die Anwendungs-Hülle ist eine Bibliothek, die ein kleines Binary mit einer Verdrahtung startet; Erweiterungspunkte im Backend und im Frontend erlauben einer anderen Edition, Funktionen hinzuzufügen, ohne dass das öffentliche Repo von ihr abhängt.
Bezüge: Spec 0037 (Entitlements), Spec 0090 (CI), Spec 0084 (Anwendungslogik und Hülle), ADR 0035, ADR 0046.

## 1. Ziel

Das öffentliche Repo ist die Grundlage; eine andere Edition hängt von ihm ab,
nie umgekehrt. Dafür ist die Hülle eine Bibliothek, und die Binaries
unterscheiden sich nur in der übergebenen Verdrahtung. Das öffentliche Repo
baut und testet eigenständig, ohne Zugang zu irgendetwas außerhalb davon.
Verhalten ändert sich durch die Aufteilung nicht.

## 2. Hülle und Verdrahtung

Die Hülle bietet einen Startpunkt, der eine **Verdrahtung** (`Wiring`)
erhält. Sie legt fest:

- den Anbieter der Berechtigungen (Spec 0037),
- Listen für KI-Anbieter, Regelquellen und Synchronisations-Umsetzungen,
- zusätzliche Tauri-Plugins (ein Hook auf dem Builder, vor dem Start),
- die Edition (`Community` oder `Official`), die nur für Anzeige und
  Aktualisierung zählt,
- zusätzliche Datenpfade, die die Einstellungen neben den eingebauten
  anzeigen.

Die Verdrahtung der Community Edition hat einen festen Berechtigungs-Satz
ohne freigeschaltete gesperrte Funktion (Spec 0037) und keine Plugins,
Synchronisations-Umsetzungen oder Zusatzpfade. Die Listen für KI-Anbieter
und Regelquellen bleiben dort leer: KI-Anbieter werden je Server-Verbindung
aus dem hinterlegten Schlüssel gebaut, der lokale Regelspeicher erst nach dem
Öffnen der Datenbank angebunden (ADR 0035).

Die Edition darf in Filter, Risiko-Einstufung und Bestätigung nie eine
eigene Verzweigung auslösen.

### Sitzungs-Erweiterung (Server-Umgebung)

Die Verdrahtung kann **eine** Sitzungs-Erweiterung enthalten; die Community
Edition hat keine. Sie liefert dem Kern je Server (Server-Kennung inklusive
des lokalen Pseudo-Servers, Gruppenkette von der Wurzel her, Tags):

- **Kontext-Abschnitt:** Text, den der Kern dem KI-Kontext der Sitzung nach
  den eigenen Absätzen anhängt, unter einer eigenen Überschrift (Deutsch und
  Englisch) und in einem eigenen Fence (Spec 0039). Der Text gilt wie eine
  Notiz als nicht vertrauenswürdig: er wird erst redigiert, dann gefenct,
  und setzt wie Notizen das Merkmal „nicht vertrauenswürdiger Inhalt
  eingelesen“. Der Abschnitt wird bei jeder Nutzernachricht neu erfragt.
  Liefert die Erweiterung nichts (oder nur Leerraum) oder gibt es keine
  Erweiterung, ist der Kontext unverändert, ohne leeren Abschnitt.
- **Bekannte Secrets:** Zeichenfolgen, die der Redactor dieses Servers
  zusätzlich zum Sudo-Passwort maskiert: in der Sitzung (Live-Ausgabe,
  gespeicherter Chat, KI-Kontext), beim Wiederverbinden, in Notizen-
  Vorschlägen und im MCP-Notizen-Abruf. Sie gelten nur für den Server, für
  den die Erweiterung sie liefert. Sie werden wörtlich verglichen, nie als
  Regex; leere Werte maskieren nichts. Die Erweiterung kann den Redactor
  offener Sitzungen eines Servers neu aufbauen lassen (etwa nach geänderten
  Werten); schon gesendeter Verlauf wird beim nächsten Senden erneut
  redigiert. Redaction wird dadurch nur erweitert, nie abgeschwächt.
- **Entfernen:** Nach dem Löschen eines Servers (auch eines nicht nutzbaren)
  und nach dem Löschen einer Gruppe, einschließlich jeder mitgelöschten
  Untergruppe, wird die Erweiterung benachrichtigt, damit sie ihre eigenen
  Daten aufräumen kann.

Die Erweiterung hat keinen Zugriff auf den Profilspeicher, nur auf die
übergebenen Angaben. Filter, Risiko-Einstufung und Bestätigung kennen sie
nicht.

**Master-Passwort-Modus** (Spec 0101): Plugins dürfen Befehle erst **nach
dem Entsperren** registrieren, sonst wären sie vorher aufrufbar.

## 3. Ein Binary je Edition

Das Community-Binary ist ein dünnes Programm, das nur die Verdrahtung der
Community Edition an den Startpunkt der Hülle übergibt. Ein weiteres Binary
für eine andere Edition gehört nicht zu diesem Repo.

## 4. Frontend-Erweiterungspunkte

Das Frontend hat ein kleines Verzeichnis für Beiträge, die eine Edition
registrieren kann. Jeder Typ hat einen tatsächlichen Renderer:

- **Einstellungs-Abschnitte** (in den Einstellungen gerendert),
- **Dokument-Aktionen** (an der Dokument-Karte im Chat, Spec 0045),
- **Aktionen im Chat-Eingabefeld** (Spec 0110 §10): Symbol-Schaltflächen
  neben „Senden“ in Registrierungsreihenfolge, per Tastatur erreichbar und
  mit zugänglicher Beschriftung; eine Aktion kann gesperrt sein (mit
  Begründung als Tooltip). Eine Aktion fügt dem Eingabefeld ausstehende
  Anhänge hinzu oder entfernt sie. Dazu kann eine Edition einen
  **Ablege-Handler** registrieren, der Dateien bekommt, die auf den Chat
  gezogen werden,
- **Erweiterungen der Erststart-Seite** (Spec 0031 §6),
- **Abschnitte im Server-Formular**: unter dem Formular eines bestehenden
  Servers, auch beim lokalen Pseudo-Server. Der Abschnitt erfährt die
  Server-ID und ob das Formular ungespeicherte Änderungen hat. Bei einem
  neuen, noch nicht gespeicherten Server erscheint kein Abschnitt,
- **Abschnitte im Gruppen-Formular**: wie beim Server, nur für bestehende
  Gruppen,
- **Ansichten im Verwalten-Tab**: ein Eintrag mit Beschriftung und Symbol
  als zusätzliche Zeile unter dem Server-Baum der Seitenleiste; ausgewählt
  zeigt er seine Komponente im Hauptbereich. Der Eintrag ist wie die
  Baumzeilen per Tastatur erreichbar.

Ob ein Beitrag gesperrt ist, entscheidet der Registrierende anhand der
Berechtigungen; die Registry kennt sie nicht. Ohne Registrierungen sehen
Formulare, Seitenleiste und Chat-Eingabefeld aus und verhalten sich wie
zuvor.

Die Beiträge werden registriert, bevor die erste Komponente rendert; eine
spätere Registrierung (nach einer asynchronen Prüfung) ist zulässig. Bei
Einstellungs-Abschnitten, Formular-Abschnitten, Verwalten-Ansichten und
Chat-Aktionen erscheint sie in einer bereits geöffneten Ansicht ohne Neuladen.

**Berechtigungen im Frontend:** Ein Hook liest den Stand der
Berechtigungen per Befehl und abonniert das Ereignis `entitlements:changed`,
das ausgelöst wird, wenn der Anbieter einen neuen Stand meldet (in der
Community Edition nie, die Infrastruktur steht trotzdem).

**Gesperrte Funktionen:** Ein `FeatureLocked`-Fehler (Spec 0037) wird
zentral abgefangen und als Hinweis-Dialog gezeigt. Der Dialog ist über allen
Ansichten sichtbar, auch bei aktivem Sitzungs-Tab. Ein vollständiger
Upgrade-Ablauf gehört nicht zu diesem Stand.

## 5. Abhängigkeitsprüfung

Die Lizenzprüfung der Rust-Abhängigkeiten (Spec 0090) ist ein Pflichtschritt
der CI.

## 6. CI aus einem frischen Klon

Die CI baut das öffentliche Repo aus einem frischen Klon ohne Geheimnisse:
Bauen, Tests, Clippy ohne Warnungen, Formatprüfung, Lizenzprüfung und
Frontend-Build auf Linux, macOS und Windows (Spec 0090). Schlägt ein
Schritt fehl, weil etwas von außerhalb des öffentlichen Repos fehlt, ist das
ein Fehler in der Trennung.

## 7. Sicherheitszusagen

Die Aufteilung ändert keine Funktion: Jeder Test, der vorher grün war, ist
es danach unverändert.

## 8. Nachweis

Der gesamte Testlauf bleibt unverändert grün; die CI läuft aus einem frischen
Klon durch.

## 9. Grenzen

- Das Frontend-Verzeichnis für Beiträge ist ein gewöhnliches Modul der
  Community-App, kein eigenes Paket (ADR 0046).
- Eine SBOM-Erzeugung und ein vollständiger Upgrade-Dialog gehören nicht zu
  diesem Stand.
