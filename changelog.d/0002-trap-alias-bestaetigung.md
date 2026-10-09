### Sicherheit
- `trap`-Handler und Alias-Definitionen (`alias NAME=WERT`) verlangen jetzt
  immer eine Bestätigung, auch wenn eine Allow-Regel greift: Sie hinterlegen
  Code, der erst später läuft. Trifft der hinterlegte Code eine Deny-Regel
  oder die Hard-Blacklist, wird das Kommando entsprechend blockiert bzw.
  gemeldet. Abfrageformen wie `trap -p` oder `alias` bleiben unverändert.
