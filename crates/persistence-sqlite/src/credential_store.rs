//! Spec 0101, A9: der produktive [`CredentialStore`] — Secrets in der
//! verschlüsselten Datenbank statt im Schlüsselbund des Betriebssystems.
//!
//! # Der synchrone Trait auf dem asynchronen Pool (Teil 0, Frage 2)
//!
//! [`CredentialStore`] ist **synchron** (`fn get`/`set`/`delete`, Spec 0003)
//! und wird aus asynchronen Tauri-Kommandos, aus dem Startablauf und aus der
//! Auflösung der SSH-Zugangsdaten heraus aufgerufen. `sqlx` ist
//! ausschließlich asynchron. Die Brücke dazwischen ist gemessen worden,
//! nicht geraten:
//!
//! | Aufrufort | `Handle::try_current()` | Weg |
//! |---|---|---|
//! | Startablauf (`build_app_state`, außerhalb jeder Runtime) | `Err` | `handle.block_on(..)` direkt |
//! | Tauri-Kommando (Task auf der Multi-Thread-Runtime) | `Ok` | `block_in_place(\|\| handle.block_on(..))` |
//! | `spawn_blocking`-Thread | `Ok` | ebenso |
//!
//! `block_in_place` meldet der Runtime, dass dieser Arbeitsthread gleich
//! blockiert, und lässt sie die übrigen Tasks auf andere Threads schieben.
//! Ohne diesen Schritt würde `block_on` innerhalb eines Runtime-Kontexts
//! panicken („Cannot block the current thread from within a runtime").
//!
//! **Kein zweiter Pool, keine zweite Verbindung, keine Trait-Änderung.** Der
//! Store teilt sich den einen Pool (`max_connections(1)`) mit allen anderen
//! Stores — aus demselben Grund wie [`crate::SqliteAiProviderStore`]: eine
//! zweite Verbindung auf dieselbe SQLCipher-Datei hieße ein zweiter
//! `PRAGMA key`-Block außerhalb von `connect_encrypted` und damit außerhalb
//! der Redaktion (A2, s. `crate::encryption::redact_if_key_bearing`).
//!
//! **Zur Verklemmung mit dem Pool der Größe 1** (gemessen): Hielte ein
//! Aufrufer die einzige Verbindung, während er hier hereinkommt, hängt
//! nichts — `sqlx` bricht nach seinem `acquire_timeout` mit „pool timed out
//! while waiting for an open connection" ab, also sichtbar statt stumm. Der
//! Fall kann heute nicht eintreten: Der Pool verlässt diese Crate nicht, und
//! jede Methode der Stores gibt ihre Verbindung innerhalb desselben `await`
//! wieder frei. Die Transaktionen in `store.rs`/`ai_provider_store.rs` rufen
//! keinen `CredentialStore` auf.
//!
//! # Die eine verbleibende Annahme, und wie sie festgenagelt ist
//!
//! `block_in_place` **panickt** auf einer `current_thread`-Runtime
//! (gemessen: „can call blocking only when running on the multi-threaded
//! runtime"). Tauris Runtime ist `MultiThread` (ebenfalls gemessen, und von
//! einem Test in `app-shell` festgehalten). Damit ein künftiger Wechsel
//! nicht als Panic vor dem ersten Fenster endet, prüft [`Self::block_on`]
//! die Variante selbst und gibt einen [`CredentialError::Backend`] zurück
//! statt zu panicken: sichtbar scheitern, nicht abstürzen.

use secrecy::{ExposeSecret, SecretString};
use sqlx::sqlite::SqlitePool;
use tokio::runtime::{Handle, RuntimeFlavor};

use ssh_manager_core::profiles::{
    CredentialError, CredentialRef, CredentialResult, CredentialStore,
};

/// Spec 0101, A9. Siehe Modul-Kommentar.
pub struct SqliteCredentialStore {
    pool: SqlitePool,
    /// Der Griff auf die Runtime, auf der die Abfragen laufen. Wird beim
    /// Aufbau mitgegeben, statt ihn bei jedem Aufruf über
    /// [`Handle::current`] zu holen: Im Startablauf gibt es gar keine
    /// laufende Runtime (gemessen), `Handle::current()` würde dort panicken.
    handle: Handle,
}

impl SqliteCredentialStore {
    pub fn new(pool: SqlitePool, handle: Handle) -> Self {
        Self { pool, handle }
    }

    /// Die Brücke aus dem Modul-Kommentar — der **einzige** Ort dieser
    /// Crate, an dem synchroner Code auf asynchronen trifft.
    fn block_on<F: std::future::Future>(&self, fut: F) -> CredentialResult<F::Output> {
        let Ok(entered) = Handle::try_current() else {
            // Kein Runtime-Kontext (Startablauf): direkt blockieren.
            return Ok(self.handle.block_on(fut));
        };
        // **Die betretene Runtime entscheidet, nicht die eigene**
        // (spec-reviewer Runde 1): `block_in_place` panickt an der Runtime,
        // in deren Kontext der Aufruf steht. Hier `self.handle` zu prüfen
        // hieße, Tauris Variante zu messen und dann an einer anderen zu
        // panicken — genau die Zusicherung „sichtbar scheitern, nicht
        // abstürzen" wäre damit nicht erfüllt, und jeder `#[tokio::test]`
        // in der Standard-Variante stürzte ab statt zu scheitern.
        if entered.runtime_flavor() != RuntimeFlavor::MultiThread {
            // Kann heute nicht eintreten (Test in `app-shell`). Falls doch:
            // ein sichtbarer Fehler statt eines Panics vor dem ersten
            // Fenster. Ohne Nutzlast aus der Bibliothek, ohne Referenz.
            return Err(CredentialError::Backend(
                "Secret-Speicher: die Laufzeitumgebung erlaubt keinen blockierenden Zugriff"
                    .to_string(),
            ));
        }
        Ok(tokio::task::block_in_place(|| self.handle.block_on(fut)))
    }

    /// Spec 0101, A9: „`Backend` ohne Secret im Text".
    ///
    /// Die Nutzlast ist der `sqlx`-Fehlertext. Er kann die Anweisung
    /// zitieren, die ihn ausgelöst hat — und die Anweisungen hier sind
    /// durchweg vorbereitet (`?`-Platzhalter), der Wert steht also nicht
    /// darin. Verlassen wird sich darauf **nicht**: Der Text wird gar nicht
    /// durchgereicht, sondern durch einen festen ersetzt. Was die Bibliothek
    /// wirklich gesagt hat, geht nur ins Log (A9.1 lässt den Code den Text
    /// ersetzen, dasselbe Muster wie Spec 0098 A5).
    fn backend_error(context: &'static str, err: &sqlx::Error) -> CredentialError {
        // **Nicht `%err`** (spec-reviewer Runde 1): Ein `sqlx`-Fehlertext
        // kann die Anweisung zitieren, die ihn ausgelöst hat — und dass das
        // auf dem geschlüsselten Weg den `PRAGMA key`-Wert bedeuten kann,
        // ist in `crate::encryption::redact_if_key_bearing` gemessen. Hier
        // steht kein Schlüssel zur Verfügung, mit dem sich redigieren
        // ließe; also geht nur die Fehlerart ins Log, nie ihr Text. Der
        // Verlust ist klein (`context` sagt, welche der drei Anweisungen es
        // war), die Zusicherung aus A2 hängt dafür nicht an einer
        // Pool-Einstellung zwei Module entfernt.
        tracing::warn!(
            from_database = err.as_database_error().is_some(),
            context,
            "secret store query failed (Spec 0101, A9)"
        );
        CredentialError::Backend(format!("Secret-Speicher: {context}"))
    }
}

/// Spec 0101, A9: „Semantik wie bisher" — dieselben drei Methoden, dieselben
/// zwei Fehlerarten, `NotFound` bleibt `NotFound`, `delete` ist idempotent.
/// Wörtlich dasselbe Verhalten wie `credentials_keyring::
/// KeyringCredentialStore`, nur mit der Datenbank als Ablage.
impl CredentialStore for SqliteCredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        let pool = self.pool.clone();
        let key = r.as_str().to_string();
        let row: Option<String> = self
            .block_on(async move {
                sqlx::query_scalar::<_, String>("SELECT value FROM secrets WHERE ref = ?")
                    .bind(key)
                    .fetch_optional(&pool)
                    .await
            })?
            .map_err(|err| Self::backend_error("Lesen fehlgeschlagen", &err))?;
        // `NotFound` bleibt `NotFound` (A9, Spec 0071 A14/I4): „kein
        // Eintrag" ist eine fachliche Aussage, keine Störung des Speichers.
        row.map(SecretString::from)
            .ok_or_else(|| CredentialError::NotFound(r.clone()))
    }

    fn set(&self, r: &CredentialRef, value: SecretString) -> CredentialResult<()> {
        let pool = self.pool.clone();
        let key = r.as_str().to_string();
        // `expose_secret` genau hier und nur hier: Der Wert geht direkt als
        // gebundener Parameter in die Anweisung, nicht in einen `format!`.
        let value = value.expose_secret().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.block_on(async move {
            // Überschreiben ist erlaubt und ersetzt den Wert — dasselbe
            // Verhalten wie `set_password` im Schlüsselbund.
            sqlx::query(
                "INSERT INTO secrets (ref, value, updated_at) VALUES (?, ?, ?) \
                 ON CONFLICT(ref) DO UPDATE SET value = excluded.value, \
                 updated_at = excluded.updated_at",
            )
            .bind(key)
            .bind(value)
            .bind(now)
            .execute(&pool)
            .await
        })?
        .map_err(|err| Self::backend_error("Schreiben fehlgeschlagen", &err))?;
        Ok(())
    }

    fn delete(&self, r: &CredentialRef) -> CredentialResult<()> {
        let pool = self.pool.clone();
        let key = r.as_str().to_string();
        // Idempotent: Ein `DELETE` auf eine nicht vorhandene Zeile ist in
        // SQL kein Fehler, und das ist hier genau das geforderte Verhalten
        // (A9) — `KeyringCredentialStore` behandelt `NoEntry` ebenso.
        self.block_on(async move {
            sqlx::query("DELETE FROM secrets WHERE ref = ?")
                .bind(key)
                .execute(&pool)
                .await
        })?
        .map_err(|err| Self::backend_error("Löschen fehlgeschlagen", &err))?;
        Ok(())
    }
}

/// Nur für Tests und den Umzug (A10): wie viele Secrets liegen in der
/// Datenbank. Kein Wert, nur die Anzahl.
impl SqliteCredentialStore {
    pub async fn count(&self) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM secrets")
            .fetch_one(&self.pool)
            .await
    }

    /// Spec 0101, A9: schließt den Pool. Nur für Tests — im Betrieb hält
    /// `SqliteProfileStore` den Pool, und der lebt bis zum Programmende.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn close(&self) {
        self.pool.close().await;
    }
}
