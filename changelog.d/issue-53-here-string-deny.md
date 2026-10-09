### Sicherheit
- Code, den eine Shell aus einem Here-String liest (`bash <<< "..."`), wird
  jetzt wie bei `bash -c "..."` geprüft: Eine Deny-Regel greift auch
  dahinter, statt dass das Kommando nur zur Bestätigung angeboten wird.
