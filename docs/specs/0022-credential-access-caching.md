# Spec: Reduzierung wiederholter Credential-Abfragen (macOS Keychain)

Status: Entwurf
Modul: `crates/app-tauri` (Session-Handling, Provider-Instanziierung),
Build-/Signing-Konfiguration
Abhängigkeiten: `CredentialStore` (Spec 0003), Session-Handling (Spec 0007),
Sudo-Passwort (Spec 0018)

## 1. Problem

Häufige macOS-Systemabfragen ("App möchte auf Schlüsselbund-Element X
zugreifen") beim Testen. Zwei mögliche, sich überlagernde Ursachen:

1. **Instabile Code-Signatur im Dev-Build.** macOS bindet eine
   "Immer erlauben"-Freigabe für ein Keychain-Item an die Code-Signatur der
   zugreifenden App. Bei `cargo tauri dev` ändert sich die Binary-Signatur
   bei jedem Neubau (ad-hoc/unsigniert) — macOS behandelt das als neue,
   nicht vertrauenswürdige App und verwirft vorherige Freigaben. Das ist
   primär ein **Dev-Zeit-Phänomen**, kein Produktionsproblem, sofern die
   App später mit einer stabilen Developer-ID signiert wird.
2. **Möglicherweise redundante Abrufe im Code.** Nicht abschließend geklärt,
   ob z. B. der AI-Provider-API-Key bei jedem `send()`-Aufruf erneut aus dem
   `CredentialStore` gelesen wird statt einmalig gecacht zu werden — im
   Gegensatz zu SSH-Login- und Sudo-Passwort, die laut Spec 0007/0018
   bereits einmalig bei `connect()` in die `Session`-Struktur geladen
   werden.

## 2. Ziel

Beide Ursachen unabhängig voneinander angehen: Caching dort nachziehen, wo
es fehlt, und die Dev-Signatur so weit wie praktikabel stabilisieren, ohne
einen vollen Developer-ID-Prozess zur Voraussetzung für lokale Entwicklung
zu machen.

## 3. Audit und Fix: Credential-Caching

Prüfen und, wo nötig, korrigieren:

- **AI-Provider-API-Key**: wird beim Aufbau der `AiProvider`-Instanz für
  eine Session (bzw. beim Setzen des aktiven Providers, Spec 0007 Abschnitt
  8) **einmalig** aus dem `CredentialStore` gelesen und für die Lebensdauer
  der Instanz gehalten — nicht pro `send()`-Aufruf erneut. Ändert sich der
  aktive Provider (`set_active_ai_provider`), wird neu gelesen, sonst nicht.
- **SSH-Login-Credential und Sudo-Passwort**: bereits laut Spec 0007/0018
  einmalig bei `connect()` gecacht — hier nur verifizieren, dass die
  Implementierung das tatsächlich einhält (kein Abruf pro `execute()`-Aufruf).
- **Host-Key-Store** und andere nicht-geheime Konfigurationsdaten sind
  ohnehin nicht im `CredentialStore`, betreffen dieses Problem also nicht.

Kein Cache über die Lebensdauer eines einzelnen App-Starts hinaus (kein
Schreiben eines eigenen sekundären Caches auf die Festplatte) — das würde
dem Prinzip "Secrets ausschließlich im OS-Schlüsselbund" (Spec 0003)
widersprechen. Der Cache ist rein In-Memory, pro laufendem Prozess.

## 4. Dev-Signatur stabilisieren

Für lokale Entwicklung: ad-hoc-Signierung mit einer **stabilen,
projektspezifischen Identität** statt einer bei jedem Build neu berechneten.
Praktisch heißt das, den Dev-Build-Schritt (bzw. das zugrunde liegende
Tauri-/Xcode-Tooling) so zu konfigurieren, dass dieselbe ad-hoc-Kennung über
mehrere `cargo tauri dev`-Läufe hinweg erhalten bleibt, statt bei jedem
Neubau neu generiert zu werden — dadurch bleibt eine einmal erteilte
"Immer erlauben"-Freigabe über mehrere Dev-Sessions hinweg gültig, auch wenn
sich der Code ändert.

Klarstellung, keine falschen Erwartungen wecken: Das reduziert die
Abfragen während der Entwicklung, ersetzt aber **nicht** eine echte
Developer-ID-Signatur für die spätere Veröffentlichung — dieser Punkt ist
rein für das lokale Entwickler-Erlebnis gedacht.

## 5. Offene Punkte

- Für die spätere Release-Signierung mit einer echten Apple Developer-ID
  entfällt Abschnitt 4 ohnehin — das ist ein eigenes, hier nicht behandeltes
  Thema (Notarisierung, Distribution).
- Falls nach Umsetzung von Abschnitt 3 immer noch häufige Abfragen
  auftreten, die sich nicht auf Punkt 1/2 zurückführen lassen: mit dem
  strukturierten Logging aus Spec 0016 lässt sich exakt sehen, welcher
  `CredentialStore::get()`-Aufruf wann und wie oft passiert — nächster
  Diagnoseschritt, falls diese Spec das Problem nicht vollständig löst.
