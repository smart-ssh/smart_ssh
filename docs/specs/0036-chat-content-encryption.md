# Spec: Feld-Verschlüsselung für persistierte Chat-Inhalte

Status: Entwurf
Modul: Erweiterung `persistence-sqlite`, `crates/app-tauri`
Abhängigkeiten: Chat-Session-Persistenz (Spec 0034), Credential-Store (Spec
0003), ursprüngliche Verschlüsselungs-Entscheidung (Spec 0004, Abschnitt 7)

## 1. Ausgangslage

Der Architektur-Brief (Business-/Lizenz-Entscheidungen, Abschnitt 12, Punkt
5) fordert eine erneute Bewertung der DB-Verschlüsselung, ausgelöst durch
die neu hinzugekommenen Session-Caches (Spec 0034) — mehr tatsächlich
sensible Konversationsinhalte liegen jetzt persistent vor als zum Zeitpunkt
der ursprünglichen Entscheidung in Spec 0004.

**Full-Database-SQLCipher ist mit unserem Stack nicht ohne Weiteres
umsetzbar**: `sqlx` (Spec 0004, Begründung: Compile-Time-Query-Checking,
natives Async) hat keine offizielle SQLCipher-Unterstützung — ein
Community-Fork musste `sqlx-sqlite` patchen, um die Inkompatibilität
(SQLCipher baut mit `SQLITE_OMIT_LOAD_EXTENSION`, kollidiert mit fest
verdrahteten Funktionsimporten) zu umgehen. Ein Wechsel zu `rusqlite`
(das SQLCipher nativ unterstützt) würde die ursprüngliche `sqlx`-Entscheidung
rückgängig machen — kein akzeptabler Kompromiss für dieses Problem.

## 2. Entscheidung: gezielte Feld-Verschlüsselung statt Full-Database

Statt der gesamten Datenbank wird ausschließlich der **Inhalt persistierter
Chat-Nachrichten** (Spec 0034, `chat_messages.content`) verschlüsselt.
Metadaten (Servernamen, Zeitstempel, Gruppenstruktur, Regelkonfiguration
etc.) bleiben wie in Spec 0004, Abschnitt 7 bewertet — nicht zusätzlich
verschlüsselt, OS-Festplattenverschlüsselung wird weiterhin als
Grundvoraussetzung angenommen.

## 3. Technologie

**`chacha20poly1305`** (reine Rust-Implementierung, RustCrypto-Projekt,
kein C-Linking, kein OpenSSL-Vendoring) für authentifizierte Verschlüsselung
(AEAD). Bleibt vollständig innerhalb der bestehenden `sqlx`-Architektur —
aus Sicht von `sqlx` ist die Spalte einfach ein Blob, keine
Sonderbehandlung, keine Kompatibilitätsprobleme.

```rust
pub struct EncryptedContent {
    pub ciphertext: Vec<u8>,
    pub nonce: [u8; 12],
}

pub trait ContentCipher: Send + Sync {
    fn encrypt(&self, plaintext: &str) -> Result<EncryptedContent, CipherError>;
    fn decrypt(&self, data: &EncryptedContent) -> Result<String, CipherError>;
}
```

Gespeichert wird `nonce || ciphertext` als ein zusammenhängender Blob pro
Zeile (`chat_messages.content` wird von `TEXT` auf `BLOB` umgestellt).

## 4. Schlüsselverwaltung

Ein einmalig generierter 256-Bit-Schlüssel, gespeichert über den
bestehenden `CredentialStore` (Spec 0003) unter einem festen Slot
(`app:chat_content_encryption_key`) — **kein neuer Speichermechanismus**,
konsistent mit "Secrets ausschließlich im OS-Schlüsselbund". Wird bei
Bedarf (erster Schreibzugriff auf `chat_messages`) automatisch generiert,
falls noch nicht vorhanden.

## 5. Migration bestehender Daten

Da `chat_messages` erst mit Spec 0034 eingeführt wird, gibt es zum
Zeitpunkt dieser Spec vermutlich noch keine unverschlüsselten Bestandsdaten
zu migrieren — falls doch (z. B. wenn Spec 0034 bereits vor dieser Spec in
Produktion war): einmaliges Migrations-Skript, das bestehende
Klartext-Zeilen liest, verschlüsselt zurückschreibt, beim App-Start
ausgeführt, idempotent (kein zweites Verschlüsseln bereits verschlüsselter
Zeilen).

## 6. Scope-Frage: weitere Tabellen?

Offen, ob zusätzlich `prompt_history.content` (Spec 0015) — ebenfalls
freier Nutzertext, potenziell sensibel — in denselben Mechanismus
einbezogen werden soll. Technisch identischer Aufwand (dieselbe
`ContentCipher`-Abstraktion), aber bewusst als offene Scope-Entscheidung
markiert statt automatisch mit einzuschließen.

## 7. Offene Punkte

- Abschnitt 6 (Einbeziehung von `prompt_history`) — Entscheidung steht aus.
- Sollte perspektivisch doch ein vollständiges Full-Database-SQLCipher
  gewünscht sein (z. B. falls sich die `sqlx`-Kompatibilitätslage ändert
  oder ein Wechsel zu `rusqlite` für die gesamte Persistenzschicht später
  doch attraktiv wird): das wäre ein deutlich größerer, eigener
  Architektur-Schritt, nicht Teil dieser Spec.
