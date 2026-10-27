# ADR 0150 — Dateiauswahl unter Android

## Status

Accepted · Issue: #679 · Spec 0114

## Kontext

Der Android-Auswahldialog liefert `content://`-URIs statt Dateipfade. Die Übertragungs-, Import- und Export-Pfade der App arbeiten mit Dateipfaden.

## Entscheidungen

1. **URIs nur über `UriOpener`.** Ein lokaler Zieltyp (`LocalFileHandle`: Pfad oder URI) wird in der Grenzschicht gebildet; nur ein `UriOpener` (unter Android über `tauri-plugin-fs`) öffnet URIs. Die Sicherheitslogik sieht keine URIs.
2. **Übertragungen über eine app-private Kopie.** Upload: die gewählte Datei wird in den app-privaten Ordner kopiert und von dort hochgeladen. Download: erst in eine app-private Datei, danach in die gewählte URI kopiert. Kopien werden nach Gebrauch gelöscht. Grund: Die SFTP-Schicht bleibt unverändert und pfadbasiert.
3. **Anzeigename aus der URI abgeleitet**, da `tauri-plugin-dialog` keinen Namen liefert. Bei IDs wie `image:123` kann der Name wenig aussagekräftig sein.
4. **`Include` in der SSH-Konfiguration** wird bei URIs nicht verfolgt (kein Verzeichnis zum Auflösen).
5. **In-App-Editor** statt externem Programm für „Lokal öffnen“, begrenzt auf 2 MiB Text, weil ein „Öffnen mit“-Intent eine Kotlin-Komponente und ein Dateifreigabe-Konzept bräuchte.
6. **Schlüssel-Import** über die Anmeldeart „Private Key“; „Schlüsseldatei“ (Pfad merken) entfällt für neue Server, da eine URI nicht dauerhaft lesbar ist.
7. Text-Lesen (Schlüssel, SSH-Konfiguration) ist auf 16 MiB begrenzt; Datenströme sind wie am Desktop unbegrenzt.

## Offen

Laufzeitverhalten wurde ohne Gerät nicht geprüft (nur Cross-Compile).
