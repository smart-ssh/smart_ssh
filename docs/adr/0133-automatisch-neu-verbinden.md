# ADR 0133 — Automatisch neu verbinden: Ablauf im Backend, Abbruch nach Erlaubnisliste

Status: angenommen · Issue: #356 · Spec 0107 §8

## Entscheidungen

1. **Der Ablauf liegt im Backend.** Der Verbindungsabbruch wird dort
   gemeldet (Terminal, Keepalive, Transportsignal); nur dort sind
   Verbindungsaufbau, Host-Key-Prüfung und KI-Runde ohne sichtbares Fenster
   erreichbar. Das Frontend zeigt nur den Stand (`auto-reconnect-changed`) und
   hat den „Stopp“-Link. Zeitplan und Schleife sind Tauri-frei in `app-logic`
   und mit angehaltener Uhr getestet.
2. **Weiterversuchen nur bei Netzfehlern.** Eine Erlaubnisliste von
   Fehlercodes (Server/Netz nicht erreichbar, Zeitüberschreitung, Kanal weg)
   wird wiederholt; jeder andere und jeder unbekannte Code beendet die
   Versuche. So bleibt ein später hinzukommender Fehlercode sicher
   (fail closed) und ein Server wird bei abgelehnter Anmeldung nicht mit
   Versuchen belastet.
3. **Ein automatischer Versuch fragt nie nach dem Host-Key.** Er nutzt
   denselben Weg wie das manuelle Neuverbinden, bricht aber bei unbekanntem
   oder geändertem Schlüssel mit `SSH_HOST_KEY_NOT_TRUSTED` ab, ohne Dialog
   und ohne den Schlüssel zu speichern.
4. **Die Einstellung steht in `settings.json`** (`autoReconnectSeconds`,
   Sekunden, 0 = aus) wie die anderen UI-Einstellungen. Das Backend liest sie
   bei jedem Abbruch; eine Änderung gilt ab dem nächsten Abbruch. Unbekannte
   Werte gelten als Standard (60 s), nie als „unbegrenzt“.
5. **Die KI-Fortsetzung ist eine normale Nutzernachricht** mit dem Hinweis
   (Sprache wie der System-Prompt). Sie läuft durch denselben Weg wie jede
   Chat-Nachricht: Läuft noch ein Turn, wird sie eingereiht. Das unterbrochene
   Kommando wird nie wiederholt, Vorschläge laufen durch Filter und Bestätigung.
   Ob ein Turn aktiv war, wird beim Abbruch festgehalten und beim Erfolg
   einmal verbraucht; ein Stopp des Nutzers (Spec 0066) verwirft die
   Fortsetzung. Der Hinweis erscheint als Nutzernachricht im Verlauf.
6. **MCP-Sitzungen** werden nicht automatisch neu verbunden (kein Tab, kein
   Nutzer-Chat).
