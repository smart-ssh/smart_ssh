# Spec 0052 — Versions- und Build-Anzeige

Status: umgesetzt
Zweck: Auf einen Blick sichtbar machen, welcher Build läuft — Version plus Commit-Hash. Jeder Bug-Report und jedes Log trägt damit die exakte Kennung, ohne dass sie erfragt werden muss.
Bezüge: Spec 0014 (Titelleiste), Spec 0016 und 0047 (Startlog), Spec 0048 (Versionierung), Spec 0049 (Titelleiste unter Windows), Spec 0050 (Einstellungen, Kategorie „Über"), Spec 0038 (Edition), ADR 0043.

Version allein reicht nicht, weil mehrere Builds dieselbe Version tragen können;
erst Version und Commit-Hash identifizieren den Stand eindeutig.

## 1. Was angezeigt wird

Überall dasselbe Format: `0.4.1 (a5b3e01)` — die Version aus der maßgeblichen
Quelle (Spec 0048) und der kurze Commit-Hash des gebauten Stands. Wo es passt,
folgen mit ` · ` die Edition (`Official` bzw. `Community`) und, bei einem
Entwicklungs-Build, die Kennung `Dev`. Dev- und Release-Builds nutzen getrennte
Datenverzeichnisse, deshalb wird nur der Dev-Build gekennzeichnet; ein Release-
Build gilt als Normalfall. Der Versions-/Hash-Teil wird an einer Stelle
gebildet und nirgends neu zusammengesetzt.

## 2. Der Commit-Hash steckt im Programm

Der Hash wird beim Bauen eingebettet; die App ermittelt ihn nie zur Laufzeit
(auf dem System des Nutzers gibt es kein Git).

- Lokal und in CI kommt der Hash gleich aus dem Git-Checkout. Ein neuer Commit
  auf demselben Branch führt zu einem neuen Hash, nicht zu einem veralteten.
- Ohne Git (z. B. Build aus einem Tarball) bricht der Build nicht ab; der Hash
  lautet dann `unknown`.
- Ein Build, der dieses Repository als Abhängigkeit einbindet, kann den Hash
  über die Umgebungsvariable `SMART_SSH_BUILD_HASH_OVERRIDE` vorgeben. Ein
  leerer Wert, ein Wert mit Steuerzeichen (z. B. Zeilenumbruch) oder einer über
  128 Zeichen wird mit einer Warnung ignoriert; dann gilt der Git-Hash. Die
  Community Edition setzt die Variable nie.
- Die Version selbst wird weiter aus der bestehenden Quelle gelesen.

## 3. Wo es erscheint

### 3.1 Log

Die erste Startzeile des Logs enthält die Version, den Hash, die Anzeigeform
`Version (Hash)`, den Build-Typ, Betriebssystem, Architektur und Datenpfad
(Spec 0047, Fund B1). Jedes an einen Bug-Report angehängte Log trägt so die
Build-Kennung. Ebenso das Diagnosepaket.

### 3.2 Einstellungen, Kategorie „Über"

Die Kategorie zeigt Version, Hash, Edition und Build-Typ in einer Zeile. Der
Text ist markierbar; ein Button „Kopieren" legt dieselbe Zeile in die
Zwischenablage. Scheitert das Kopieren, erscheint eine sichtbare Meldung und
der Text bleibt markierbar. Lässt sich die Information nicht laden, steht dort
eine Fehlermeldung. Zusätzlich öffnet die Kategorie die Liste der
Drittlizenzen.

### 3.3 Titelzeile (0.x-Phase)

Die Titelzeile zeigt neben „Smart SSH" die Version mit Hash, die Edition und
bei Dev-Builds `Dev`, gefolgt von „— Early Access", z. B.
`Smart SSH 0.4.1 (a5b3e01) · Official — Early Access`. Wer mehrere Builds
parallel testet, sieht so sofort, welcher läuft.

- Der gesamte Zusatz hängt an einem einzigen Schalter, der an die Early-
  Access-Kennzeichnung gekoppelt ist und für 1.0 in einem Schritt abgeschaltet
  werden kann; dann steht dort nur „Smart SSH".
- In schmalen Fenstern (unter 1024 px Breite) entfällt der Zusatz bis auf die
  Dev-Markierung, damit er die Tab-Leiste nicht bedrängt; bei Dev-Builds steht
  dann `Smart SSH · Dev`. Version, Hash und Edition bleiben unter
  Einstellungen > Über (3.2) abrufbar.
- Lässt sich die Information nicht laden, steht nur „Smart SSH".
- Der Text ist Teil der Ziehfläche der Leiste und stört weder die Fenster-
  Buttons noch das Ziehen (Spec 0049).

## 4. Grenzen

- Keine fortlaufende Build-Nummer; der Commit-Hash ist die Kennung.
- Kein Bezug zu Auto-Update, keine Telemetrie — rein lokale Anzeige.

## 5. Editionen

Einbettung, Log, Über-Kategorie und Titelzeile gehören zum öffentlichen Kern
und gelten für jede Edition. Die Edition kommt aus der Verdrahtung der
jeweiligen Binary (Spec 0038); nur ihr Wert unterscheidet die Editionen.
