# Spec: Fataler Startfehler zeigt immer ein UI

Status: Entwurf
Repo: **öffentlich** `smart_ssh` (Startup-Pfad, `app-shell`/`main`)
Modul: Der früheste Startup-Pfad, vor/während der Tauri-Initialisierung
Abhängigkeiten: SQLite-Migrationen (0004/0047), Keychain/Secret-Service-Zugriff
(0004/0036), Startup-Logging (0047 B1), Panic-Hook (0047 B1)
Release-Gate: **A (MUSS)** — ohne diesen Punkt kein Launch.

> **Das Problem:** Es gibt aktuell **keinen** Fehlerpfad für „die App kann gar
> nicht erst starten". Ein Panic *vor* dem ersten Fenster führt nur zu kurzem
> Aufblitzen ohne Fenster — für einen Doppelklick-Nutzer völlig
> undiagnostizierbar (die App „geht einfach nicht"). Für ein Produkt zum
> Herunterladen+Doppelklicken ist das der schlechteste erste Eindruck.
> **Priorität ERHÖHT** (Startphase, plattformspezifisch, berührt die
> „kein Fehlerpfad bricht ab statt sichtbar behandelt"-Invariante — die für
> die *laufende* App gilt, aber für die *Startphase* fehlt).

## Die vier Fehlerfälle (Release-Gate A)

Ein **nativer OS-Dialog** (nicht ein Tauri-Fenster — das steht ja noch nicht)
muss erscheinen bei:
1. **DB von neuerer Version angelegt** (Downgrade-Fall): älteres Binary trifft
   neuere DB. Meldung sinngemäß: „Die Datenbank wurde von einer neueren
   Version angelegt (Version X, dieses Programm kennt bis Y). Bitte die
   neueste Version installieren." + Datenpfad.
2. **DB korrupt / nicht lesbar.** Meldung + Pfad + nächster Schritt (Backup
   einspielen / Support).
3. **Keychain / Secret Service beim Start gesperrt oder nicht vorhanden.**
   (Das ist derselbe Fund wie der `.expect()`-Panic aus Spec 0040 Teil 6
   Punkt 1 — **dabei entfernen**.) Auf Linux: welches Paket fehlt
   (gnome-keyring / kwallet / KeePassXC-Secret-Service).
4. **Datenverzeichnis nicht schreibbar** (Rechteproblem). Meldung + Pfad.

Jeder Dialog: **verständlich, mit Pfadangabe und nächstem Schritt** — kein
Aufblitzen ohne Fenster, kein stiller Absturz.

## Teil 0 — ZUERST Diagnose (die eigentliche Herausforderung)

Bevor du baust, kläre und berichte mir:
- **Wo genau im Startup** tritt jeder der vier Fälle auf? (Migration bei
  `lib.rs:52`-Umfeld? Keychain-Zugriff wann? Datenverzeichnis-Anlage wann?)
- **Wie früh** ist das jeweils — vor oder während der Tauri-Builder-
  Initialisierung? Das bestimmt, ob ein nativer Dialog dort überhaupt möglich
  ist.
- **Welcher native Dialog-Mechanismus** pro Plattform: macOS `NSAlert` (oder
  eine plattformübergreifende Crate wie `rfd`/`native-dialog`?), Windows
  `MessageBox`, Linux (GTK/`zenity`/Crate?). **Eine plattformübergreifende
  Crate wäre einfacher als drei native Wege** — prüfe, ob eine geeignete
  existiert, die *ohne* laufendes Tauri-/Fenster-System funktioniert (ganz
  früh im `main`).
- Melde mir deinen Plan, bevor du die vier Fälle verdrahtest.

## Teil 1 — Nativer Dialog-Mechanismus

Ein Mechanismus, der **so früh wie möglich** im `main`/Startup einen nativen
Fehlerdialog zeigen kann, **unabhängig vom Tauri-Fenster-System**. Wähle den
saubersten Weg aus Teil 0 (bevorzugt eine plattformübergreifende Crate, falls
eine ohne Fenster-System funktioniert; sonst plattformspezifisch mit
`#[cfg(...)]`).
- Der Dialog blockiert, zeigt die Meldung, und die App **beendet sich danach
  sauber** (kein Weiterlaufen in einen kaputten Zustand).
- **Logging zuerst**: Der Fehler wird *auch* geloggt (der 0047-B1-Log läuft
  ja schon als Allererstes) — der Dialog ist zusätzlich, nicht statt.

## Teil 2 — Die vier Fälle verdrahten

Jeden der vier Startup-Fehlerpfade so umbauen, dass er **nicht paniced/still
abbricht**, sondern den nativen Dialog (Teil 1) mit der passenden Meldung
zeigt und dann sauber beendet.
- **Fall 3 (Keychain)**: den `.expect()`-Panic aus Spec 0040 Teil 6 Punkt 1
  **entfernen** und durch diesen Pfad ersetzen.
- **Fall 1 (DB-Downgrade)**: die Versionsnummern (angelegt mit X, Binary kennt
  bis Y) in die Meldung.

## Invarianten / Sicherheit
- Kein Secret/kein sensibler Inhalt im Dialog-Text (nur Fehlerart + Pfad +
  Schritt) — die Meldungen sind generisch, kein DB-Inhalt, kein Key.
- Der Dialog ändert **nichts** an den Daten (kein „automatisch reparieren" —
  nur melden + beenden; Reparatur ist Nutzer-/Support-Sache).
- Logging läuft weiter zuerst (0047 B1) — der Dialog ist die *sichtbare*
  Ergänzung.
- Kein Absturz ohne Dialog mehr in diesen vier Fällen.

## Testbarkeit
- Jeder der vier Fälle **künstlich herbeigeführt** → verständlicher Dialog
  mit Pfad + nächstem Schritt, kein Aufblitzen ohne Fenster:
  - DB von neuerer Version: eine DB mit höherer Schema-Version anlegen, altes
    Binary starten.
  - DB korrupt: DB-Datei mit Müll überschreiben.
  - Keychain gesperrt/fehlt: Linux-VM ohne Keyring; macOS mit gesperrter
    Keychain.
  - Datenverzeichnis nicht schreibbar: Rechte entziehen.
- Automatisierbar, soweit möglich (der Dialog selbst ist schwer im
  Headless-Test — mind. die *Fehlererkennung* + dass der richtige
  Dialog-Aufruf mit dem richtigen Text erfolgt, unit-testbar machen; die
  visuelle Bestätigung macht Stefan pro Plattform).

## Abschluss
- Melde mir: den Diagnose-Befund (Teil 0 — wo/wie früh, welcher Mechanismus),
  ob eine plattformübergreifende Crate reicht oder es plattformspezifisch
  wird, und je Fall einen manuellen Testablauf für Stefan (pro Plattform, weil
  der native Dialog nur echt sichtbar ist).
- Volle Gates grün. `spec-reviewer` ERHÖHT (Startphase, Panic-Entfernung,
  plattformspezifischer Code).
- CHANGELOG-Eintrag (nutzerrelevant: „App zeigt jetzt bei Startfehlern eine
  verständliche Meldung statt stillem Absturz").
