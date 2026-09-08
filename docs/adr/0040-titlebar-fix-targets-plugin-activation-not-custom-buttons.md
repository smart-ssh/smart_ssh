# 0040-titlebar-fix-targets-plugin-activation-not-custom-buttons

## Status
Akzeptiert

## Kontext

Spec 0049, Fund 3/4 beschreibt zwei Windows-Symptome aus dem ersten echten
Windows-Test: nur ein Schließen-Button in der Titelleiste (kein Minimieren/
Maximieren), und das Fenster lässt sich nicht an der Titelleiste ziehen. Die
Spec-Formulierung legt nahe, dass die Lösung eigene, in React gerenderte
Windows-Fenster-Buttons sind, die "die tatsächlichen Tauri-Fenster-Kommandos
auslösen (`minimize`, `maximize`/`unmaximize`, `close`)".

Spec 0014 (custom Titelleiste, die diese Spec 0049 explizit referenziert)
legt aber schon in Abschnitt 1/2 fest: **kein Eigenbau dieser Buttons.** Das
Projekt nutzt bewusst `tauri-plugin-decoration`, das laut eigener
Dokumentation (`~/.cargo/registry/.../tauri-plugin-decoration-3.0.5/README.md`
und `docs/integration.md`) Windows- und Linux-Fenster-Controls **selbst**
als HTML rendert und intern mit einem eigenen, geschlossenen Tauri-Kommando
verdrahtet — inklusive Windows-11-Snap-Layout-Flyout und GTK-Theme-Anpassung
auf Linux. Die Berechtigungen dafür (`decoration:default`,
`core:window:allow-start-dragging`, `core:window:allow-internal-toggle-maximize`)
sind in `apps/smart-ssh-community/capabilities/default.json` bereits korrekt
gesetzt.

Der bestehende `create_overlay_titlebar`-Tauri-Command
(`crates/app-shell/src/commands.rs`) rief `window.activate_decoration()` auf
und **verwarf das Ergebnis mit `let _ =`** — weder Fehlerbehandlung noch
Fallback noch Logging. Genau das dokumentierte Recovery-Muster des Plugins
("Activate and recover", `docs/integration.md#5-activate-and-recover")
verlangt bei einem Fehler explizit `restore_decoration()` **plus** ein
sichtbares Fenster, sonst bleibt das Fenster in einem nicht spezifizierten
Zwischenzustand — ein sehr plausibler Erklärungsansatz für genau "nur ein
Button" und "Ziehen greift nicht": ein Aktivierungsfehler, der bislang
spurlos verschluckt wurde.

**Wichtige Einschränkung**: Ich kann diese Hypothese nicht auf echter
Windows-/Linux-Hardware verifizieren (Entwicklungsumgebung ist macOS). Die
Spec selbst sieht das vor ("Fund 3/4 sind schwer automatisiert testbar ...
plus manuelle Windows-Verifikation durch Stefan").

## Entscheidung

Der Fix zielt auf die **Aktivierungs-Logik**, nicht auf eigene React-Buttons:

1. `create_overlay_titlebar` behandelt das Ergebnis von
   `activate_decoration()`/`set_traffic_lights_inset()` jetzt explizit: bei
   einem Fehler wird `restore_decoration()` aufgerufen (bringt zuverlässig
   die native Titelleiste zurück, inkl. deren eigener, garantiert
   funktionierender Minimieren-/Maximieren-/Schließen-Controls) und der
   Fehler wird geloggt (`tracing::warn!`/`tracing::error!`) statt
   verschluckt.
2. Der Command liefert jetzt `"custom"` oder `"native"` zurück; das Frontend
   (`AppHeader.tsx`) hält diesen Modus in State und reserviert nur dann
   Platz für die Plugin-Controls, wenn sie tatsächlich aktiv sind — im
   `"native"`-Fallback zeichnet das Betriebssystem seine eigene Titelzeile
   oberhalb, eine reservierte Lücke im eigenen Header wäre dort falsch.
3. **Keine eigenen Windows-Fenster-Buttons gebaut** — das würde Spec 0014s
   ausdrücklicher Entscheidung widersprechen und Funktionalität duplizieren
   (Snap-Layout-Flyout, Theme-Anpassung), die das Plugin bereits liefert.
4. Kein Eingriff in `tauri.conf.json`s Sichtbarkeits-/`decorations`-Werte
   (kein `"visible": false` + expliziter `window.show()`-Tanz, wie ihn das
   Plugin optional empfiehlt) — das wäre ein deutlich invasiverer Eingriff
   mit echtem Regressionsrisiko (ein nie sichtbar werdendes Fenster, falls
   die Aktivierung aus einem unvorhergesehenen Grund nie feuert) für ein auf
   macOS/Linux bereits funktionierendes Setup. Diese Spec verbietet
   ausdrücklich, macOS/Linux durch eine Änderung zu brechen.

## Konsequenzen

- Der eigentliche Root Cause auf Windows bleibt bis zur manuellen
  Verifikation durch Stefan unbestätigt. Sollte `activate_decoration()`
  dort tatsächlich fehlschlagen, sorgt dieser Fix jetzt wenigstens für einen
  sauberen, funktionierenden Fallback (volle native Titelleiste) statt des
  vorherigen kaputten Zwischenzustands — und für eine Logzeile, die beim
  nächsten Windows-Test zeigt, ob das die Ursache war.
- Sollte `activate_decoration()` auf Windows tatsächlich **erfolgreich**
  zurückkehren (die Aktivierung an sich funktioniert, das Problem liegt
  woanders — z. B. einem Rendering-Bug im Plugin selbst bei dieser
  Windows-Version/-Konfiguration), behebt dieser Fix das gemeldete Symptom
  NICHT. In diesem Fall braucht es einen Report ans Plugin-Upstream-Projekt
  oder doch einen Eigenbau — das wäre dann ein bewusster Bruch mit Spec
  0014s "kein Eigenbau"-Entscheidung und sollte als neue, eigene Spec
  behandelt werden, nicht stillschweigend in einem Nachbesserungs-Fix.
- Die neue Logzeile bei einem Aktivierungsfehler ist der entscheidende
  nächste Diagnose-Schritt, den es vorher nicht gab.
