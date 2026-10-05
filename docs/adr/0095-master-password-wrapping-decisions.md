# ADR 0095 — Entscheidungen beim Verpacken des Wurzelschlüssels (Spec 0101, Etappe 3)

Status: angenommen · Spec: `docs/specs/0101-database-encryption.md` (A13–A19) ·
Backlog: BL-0203, BL-0314

Protokoll der Entscheidungen, die Spec 0101 für Etappe 3 offen gelassen hat
oder die vom Wortlaut abweichen. Die Begründungen stehen zusätzlich am Code;
hier sind sie an einer Stelle nachlesbar.

## 1. Eigenes Dateiformat statt PHC-String

A14 verlangt eine versionierte Datei mit Argon2id-Parametern, Salt, Nonce
und Chiffrat, „Kopf als AAD“. Umgesetzt als festes Binärformat
(`core/src/crypto/key_wrapping.rs`): Marke `SSHKWRP\0`, Formatversion,
KDF-Kennung, m/t/p als `u32` (little endian), Salt-Länge, Salt, Nonce,
Chiffrat samt Tag.

**Warum kein PHC-String:** Ein Format, das die Parameter aus der Datei
übernimmt, nimmt auch die Parameter eines Angreifers. Die Parameter müssen
in der Datei stehen, damit sie später erhöht werden können — sie werden
deshalb gegen eine Untergrenze geprüft **und** gehen als AAD in die
Authentifizierung ein.

## 2. Obergrenzen für die Argon2-Parameter (nicht in A14 gefordert)

m ≤ 1 GiB, t ≤ 16, p ≤ 16, geprüft **vor** der Ableitung.

**Grund:** Die Parameter stehen in einer Datei, die jemand mit Schreibzugriff
auf das Datenverzeichnis ändern kann. Ohne Obergrenze wäre `m = 4 GiB` eine
Speicherbombe, die vor dem ersten Fenster zuschlägt — und die Reihenfolge ist
nicht umkehrbar: Die Authentifizierung braucht den abgeleiteten Schlüssel.
Eine Grenze nach oben ist die einzige Stelle, an der dieser Fall abzufangen
ist. Sie liegt weit über den Schreibparametern, behindert also keine
Erhöhung. Der Reviewer hält fest, dass 1 GiB noch reichlich ist; das Senken
auf z. B. 256 MiB ist Härtung, kein Fehler, und bleibt offen.

## 3. Neuer Typ `RootKey` statt `Zeroizing<[u8; 32]>`

A19 verlangt Typen, die beim Freigeben überschrieben werden. `Zeroizing`
täte das, erbt aber das `Debug` seines Inhalts — ein `?key` in einer
`tracing`-Zeile hätte K als Byte-Liste ausgegeben. `RootKey` überschreibt
beim Freigeben **und** zeigt nichts.

**Nicht erfüllt bleibt A19 (SOLL) für `RootKeyAccess::Unlocked([u8; 32])`:**
Dort wird K in ein gewöhnliches Array kopiert, weil der Bestand
(`read_key_state`, `DatabaseKey::from_root_key`) mit `[u8; 32]` arbeitet.
Keine neue Lücke, aber auch keine Erfüllung. Ein Weg dieser Bytes in Log,
DTO oder Diagnosepaket ist nicht vorhanden (kein `Debug`, kein `Serialize`).

## 4. Abweichung von der Reihenfolge in A5 (Passwort-Modus)

A5 schreibt: „das neue Master-Passwort wird **zuerst** eingerichtet (A13),
die neue Verpackungsdatei geschrieben, erst dann werden die alten Dateien
umbenannt.“

Umgesetzt ist: Passwort abfragen → alte Dateien umbenennen → neue Verpackung
schreiben.

**Grund:** Beide Dateien tragen denselben Namen. Die neue Verpackung an ihren
Platz zu schreiben, *bevor* die alte weg ist, hieße sie zu überschreiben —
und A5 verlangt im selben Satz „löscht nichts“ und „nie überschreiben“. Der
abbrechbare Teil des Einrichtens (die Passworteingabe) liegt weiter vor jedem
`rename`: Bricht der Nutzer ab, ist nichts verändert. Die Zusicherung aus A5,
auf die es ankommt, bleibt damit erhalten; nur die Satzreihenfolge nicht.

**Folge, die offen bleibt:** Scheitert das Schreiben der neuen Verpackung
nach den Umbenennungen, liegen die alten Dateien umbenannt da und es gibt
keine Verpackung. Der Text zu `MasterPasswordSetupFailed` sagt dann „es ist
nichts verändert“, was in diesem Pfad nicht stimmt. Der Pfad ist heute nicht
erreichbar (s. Punkt 7); mit seiner Freischaltung muss der Text einen eigenen
Fall bekommen.

## 5. `StartOverKey::Keep` — die Verpackungsdatei bleibt bei D2 stehen

A5 nennt die Verpackungsdatei im Umbenennungssatz als MUSS. Sie wird
trotzdem **nur** mitgenommen, wenn sie selbst das Unbrauchbare ist (D3,
`UnusableWrapping`).

**Grund:** D2 aus dem Feld *sonst* × *da* heißt „die Datei passt nicht zu
diesem Schlüssel“ — der Schlüssel ist in Ordnung. Die Verpackung dort
umzubenennen wäre der Verlust von K: Die Datenbank wird neu angelegt, und
ohne Verpackung gäbe es kein Passwort mehr, mit dem sie sich öffnen ließe.
Strenger als der Wortlaut wäre hier schädlich. Der Reviewer hat die
Begründung geprüft und keine schädliche Nebenwirkung gefunden.

## 6. Der Umzugs-Dialog ist eine eigene Dialogart

A11 sagt „Dialog D1 ohne Einrichten“. Umgesetzt als eigene Variante
`StartupDialog::MigrationUnreadable`, nicht als `D1 { offers_password_setup:
false }`. Dadurch kann dieser Dialog das Einrichten per Konstruktion nicht
anbieten (T13: „Umzugs-Dialog (A11) nie“), statt an einem `false` zu hängen,
das jemand später anders setzt. Die Texte sind dieselben.

## 7. Bewusst nicht behoben in diesem Schritt

- **Commit 11 (Oberfläche) fehlt** — Budget, nicht Fachlichkeit. Der dritte
  D1-Knopf („Master-Passwort einrichten“) wird deshalb nativ noch nicht
  gezeigt: Der Weg dahinter ist fertig, nur die Maske fehlt, und ohne sie
  stünde der Nutzer vor einem Fenster ohne Eingabefeld.
- **Kein Ausweg aus einer unbrauchbaren Verpackungsdatei** (Review-Fund 1,
  `review-05.md`): `RootKeyAccess::UnusableWrapping` wird nirgends
  konstruiert, und die Positivliste des Tors enthält kein Kommando für „Neu
  anfangen“. Die Spalte *ungültig* der Tabelle A3 ist im Passwort-Modus
  damit unerreichbar, und die dafür geschriebene Logik (A5 im
  Passwort-Modus, D3/D4) ist unerreichbar und untestet. Der abgegebene Stand
  ist dadurch nicht akut gefährlich — in den Passwort-Modus kommt man ohne
  Commit 11 nicht —, aber Etappe 3 ist damit **nicht** fertig. Gehört vor
  Commit 11.
- Die weiteren zurückgestellten Punkte des Reviewers (Vergleich vor dem
  `rename` beim Passwortwechsel, `.new`-Reste, `fsync` auf das Verzeichnis,
  Zeitgrenze für `startup:prompt`, Serialisierung des Entsperrens, das Tor
  über die Plugin-Kommandos, der Pfad im `HostKeyStoreFailed`-Text, K aus dem
  offenen Zustand beim Einrichten) stehen wörtlich in `review-05.md` und im
  Bericht.
