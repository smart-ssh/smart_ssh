//! Spec 0101, A4/A6–A8: die Datenbankdatei mit ihrem Schlüssel öffnen und
//! eine vorhandene Klartext-Datei einmalig umwandeln.
//!
//! **Hier und nicht in `store.rs`:** Der Zustand der *Datei* (A3) und die
//! Umwandlung (A6) sind Entscheidungen, die **vor** dem ersten Öffnen und
//! vor jeder Migration fallen — sie gehören nicht in einen Typ, der einen
//! bereits offenen Pool darstellt. `app-shell` fährt mit diesen Funktionen
//! die Entscheidungstabelle A3, ohne selbst `sqlx` zu kennen.
//!
//! **Was hier nie passiert:** Keine Funktion dieses Moduls erzeugt einen
//! Schlüssel, benennt eine Datei um oder löscht eine Nutzdatei, solange der
//! Nutzer nicht gewählt hat (A3, letzter Absatz). Die Umwandlung (A6) ist
//! die einzige Ausnahme und sie ist genau dann erlaubt, wenn die
//! Entscheidungstabelle sie ausdrücklich vorsieht.

use std::path::{Path, PathBuf};

use secrecy::ExposeSecret;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection, Row, SqliteConnection};

use ssh_manager_core::crypto::DatabaseKey;

/// Die 16 Kopfbytes einer **unverschlüsselten** SQLite-Datei. Eine
/// SQLCipher-Datei beginnt stattdessen mit ihrem Zufalls-Salt, kann diese
/// Folge also nur mit Wahrscheinlichkeit 2^-128 zufällig tragen.
pub const SQLITE_PLAINTEXT_HEADER: &[u8] = b"SQLite format 3\0";

/// Spec 0101, A3: Zustand der Datenbankdatei, **am Header erkannt, ohne die
/// Datei zu ändern**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseFileState {
    /// Keine Datei, oder 0 Byte **ohne** nichtleere `-wal` (A3, wörtlich).
    Missing,
    /// Die Datei beginnt mit [`SQLITE_PLAINTEXT_HEADER`].
    Plaintext,
    /// Alles andere: eine verschlüsselte Datei, eine beschädigte Datei, oder
    /// eine 0-Byte-Datei **mit** nichtleerer `-wal`.
    ///
    /// Dass der letzte Fall hier landet und nicht bei [`Self::Plaintext`],
    /// ist Absicht (A3 zählt ihn ausdrücklich nicht zu *fehlt*): Eine
    /// 0-Byte-Hauptdatei neben einem nichtleeren WAL ist nicht entscheidbar
    /// — sie kann eine frische Klartext-Datenbank sein, deren erste
    /// Transaktion noch im WAL steht, oder der Rest eines abgebrochenen
    /// Vorgangs. Statt zu raten und automatisch umzuwandeln, führt dieser
    /// Zustand über die Tabelle A3 in einen Dialog, in dem der Nutzer
    /// entscheidet; angefasst wird dabei nichts.
    Other,
}

/// A3: ermittelt den Dateizustand, ohne die Datei zu verändern (nur Lesen
/// der ersten 16 Byte; Inhalt und mtime bleiben gleich — T8 prüft das).
///
/// Ein Lesefehler, der **nicht** „nicht vorhanden" ist (z. B.
/// `PermissionDenied`), wird nicht zu *fehlt* verharmlost, sondern
/// weitergegeben — sonst würde die Tabelle A3 auf „neu anlegen" laufen und
/// eine unlesbare, aber vorhandene Datenbank überschreiben wollen.
pub fn detect_database_file_state(db_path: &Path) -> std::io::Result<DatabaseFileState> {
    let metadata = match std::fs::metadata(db_path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DatabaseFileState::Missing)
        }
        Err(err) => return Err(err),
    };

    if metadata.len() == 0 {
        let wal_is_non_empty = std::fs::metadata(wal_path(db_path))
            .map(|m| m.len() > 0)
            .unwrap_or(false);
        return Ok(if wal_is_non_empty {
            DatabaseFileState::Other
        } else {
            DatabaseFileState::Missing
        });
    }

    let mut header = [0u8; 16];
    {
        use std::io::Read;
        let mut file = std::fs::File::open(db_path)?;
        match file.read_exact(&mut header) {
            Ok(()) => {}
            // Kürzer als 16 Byte: kein Klartext-Header, also *sonst* — nicht
            // *fehlt*, denn die Datei ist vorhanden und nicht leer.
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(DatabaseFileState::Other)
            }
            Err(err) => return Err(err),
        }
    }

    Ok(if header == SQLITE_PLAINTEXT_HEADER {
        DatabaseFileState::Plaintext
    } else {
        DatabaseFileState::Other
    })
}

/// Warum die Umwandlung (A6) nicht gelaufen ist. Enthält **nie**
/// Schlüsselmaterial: die Varianten tragen entweder keinen Text oder einen
/// durch [`scrub_key_material`] geführten (s. dort).
#[derive(Debug)]
pub enum ConversionFailure {
    /// A6, letzter Satz: `smart-ssh.db` ist ein Symlink — es wird **nicht**
    /// umgewandelt, nichts verändert (T20). Grund: Die Umwandlung endet mit
    /// einem `rename` an die Stelle der Datei; das würde den Symlink
    /// ersetzen und die Zieldatei im Klartext liegen lassen.
    Symlink,
    /// Datei- oder Verzeichnisfehler.
    Io(std::io::Error),
    /// SQLite-Fehler, dessen Text kein Schlüsselmaterial enthielt.
    Database(sqlx::Error),
    /// Die Prüfung aus Schritt 3 ist fehlgeschlagen — der Text benennt den
    /// Unterschied (Tabellenname, Zeilenzahlen), nie einen Zelleninhalt.
    Verification(String),
    /// Ein Fehlertext enthielt Schlüsselmaterial und wurde deshalb
    /// vollständig verworfen (A2: der Schlüssel erscheint in keinem
    /// Fehlertext).
    RedactedDatabaseError,
}

impl std::fmt::Display for ConversionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversionFailure::Symlink => write!(
                f,
                "die Datenbankdatei ist eine symbolische Verknüpfung und wird nicht umgewandelt"
            ),
            ConversionFailure::Io(err) => write!(f, "Dateizugriff fehlgeschlagen: {err}"),
            ConversionFailure::Database(err) => write!(f, "SQLite-Fehler: {err}"),
            ConversionFailure::Verification(what) => {
                write!(f, "Prüfung der umgewandelten Datei fehlgeschlagen: {what}")
            }
            ConversionFailure::RedactedDatabaseError => write!(
                f,
                "SQLite-Fehler (Details ausgelassen, weil sie Schlüsselmaterial enthielten)"
            ),
        }
    }
}

impl std::error::Error for ConversionFailure {}

impl From<std::io::Error> for ConversionFailure {
    fn from(err: std::io::Error) -> Self {
        ConversionFailure::Io(err)
    }
}

/// A2, Angriffsrichtung „ein SQL-Fehlertext mit dem `PRAGMA key`-Wert":
/// prüft, ob `text` die Hex-Form des Schlüssels enthält.
///
/// **Das ist kein theoretischer Fall, sondern ein gemessener.** Beim Bauen
/// dieses Commits lieferte `sqlx` für ein fehlerhaft gesetztes Schlüssel-
/// Pragma wörtlich `near "x'7c83…'": syntax error` — der vollständige
/// Datenbankschlüssel im Fehlertext, der von dort über
/// `PersistenceError::Display` in Log und Startdialog gelaufen wäre. Jeder
/// Fehler, der aus dem Verbindungsaufbau mit Schlüssel stammt, läuft
/// deshalb durch diese Prüfung.
///
/// Der Vergleich läuft gegen den Hex-Anteil **ohne** die `x'…'`-Klammern,
/// damit auch eine Fassung erkannt wird, die nur die Ziffern durchreicht;
/// zusätzlich gegen die Großschreibung.
pub(crate) fn contains_key_material(text: &str, key: &DatabaseKey) -> bool {
    let pragma = key.pragma_value();
    let pragma = pragma.expose_secret();
    let hex = pragma
        .strip_prefix("x'")
        .and_then(|rest| rest.strip_suffix('\''))
        .unwrap_or(pragma);
    // spec-reviewer Runde 1: Vorher wurde nur die **vollständige**
    // 64-Zeichen-Folge gesucht. Ein Fehlertext, der die Anweisung gekürzt
    // zitiert (`near "x'7c83c8e1..."`) oder einen Umbruch einfügt, wäre
    // damit durchgegangen -- mit einem Schlüsselpräfix darin. Geprüft
    // wird deshalb **jedes** Fenster von 16 Hex-Zeichen (64 Bit
    // Schlüsselmaterial): So viel preiszugeben verkleinert den Suchraum
    // schon unzulässig, und 16 Zeichen sind lang genug, dass ein
    // zufälliger Treffer in einem Fehlertext ausgeschlossen ist.
    // 8 statt 16 Hex-Zeichen (spec-reviewer Runde 2): Das halbiert den
    // Teil-Austritt, den die Prüfung durchlässt, auf 32 Bit — und die
    // Wahrscheinlichkeit, dass ein gewöhnlicher Fehlertext zufällig acht
    // bestimmte Hex-Zeichen in Folge enthält, liegt bei ~10^-8. Ein
    // Fehler verliert dadurch praktisch nie seine diagnostische Variante.
    const WINDOW: usize = 8;
    let lower = text.to_ascii_lowercase();
    hex.as_bytes()
        .windows(WINDOW)
        .any(|w| lower.contains(std::str::from_utf8(w).expect("Hex ist ASCII")))
}

/// A2: Nimmt einem [`crate::PersistenceError`] seine Variante, wenn sein
/// Text den Schlüssel enthält. Alles andere behält sie — ein
/// `PermissionDenied` bleibt als solches erkennbar (Spec 0059, Fall 4).
pub(crate) fn redact_if_key_bearing(
    err: crate::PersistenceError,
    key: &DatabaseKey,
) -> crate::PersistenceError {
    if contains_key_material(&err.to_string(), key) {
        crate::PersistenceError::RedactedConnect
    } else {
        err
    }
}

/// Enthält der Fehlertext den Schlüssel, wird der **ganze** Text verworfen
/// statt nur die Fundstelle ersetzt — ein teilweise maskierter Text würde
/// Länge und Lage verraten und wäre eine Einladung, die Maskierung später
/// wieder „nur ein bisschen" zu lockern.
fn scrub_key_material(err: sqlx::Error, key: &DatabaseKey) -> ConversionFailure {
    if contains_key_material(&err.to_string(), key) {
        ConversionFailure::RedactedDatabaseError
    } else {
        ConversionFailure::Database(err)
    }
}

fn wal_path(db_path: &Path) -> PathBuf {
    sibling(db_path, "-wal")
}

fn sibling(db_path: &Path, suffix: &str) -> PathBuf {
    let mut name = db_path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Die Zwischendatei aus A6 Schritt 2 — im **Datenverzeichnis** neben der
/// Datenbank (§5), damit das abschließende `rename` auf demselben
/// Dateisystem liegt und damit tatsächlich atomar ist. Ein Pfad in `/tmp`
/// wäre es nicht.
pub fn intermediate_path(db_path: &Path) -> PathBuf {
    sibling(db_path, ".sqlcipher-new")
}

/// Entfernt eine liegengebliebene Zwischendatei samt ihrer Journaldateien.
///
/// A6: „Eine liegengebliebene Zwischendatei wird beim nächsten Start
/// verworfen, nie übernommen" (T19). Deshalb steht dieser Aufruf am
/// **Anfang** der Umwandlung und nicht am Ende: Was dort liegt, kann aus
/// einem abgebrochenen Lauf stammen — oder von irgendwem anders. In beiden
/// Fällen ist es kein Ergebnis, dem zu trauen wäre.
fn discard_intermediate(db_path: &Path) {
    let tmp = intermediate_path(db_path);
    // spec-reviewer Runde 1: `-journal` mit -- ein fremdes
    // `...sqlcipher-new-journal` wäre sonst liegen geblieben und könnte
    // beim Öffnen der Zwischendatei angewandt werden.
    for path in [
        tmp.clone(),
        sibling(&tmp, "-wal"),
        sibling(&tmp, "-shm"),
        sibling(&tmp, "-journal"),
    ] {
        // `remove_file` auf einem Symlink entfernt die Verknüpfung, nicht
        // ihr Ziel — eine fremde Verknüpfung an dieser Stelle kann also
        // nicht dazu führen, dass irgendwo anders etwas gelöscht wird.
        let _ = std::fs::remove_file(path);
    }
}

/// Reihenfolgeschritt aus A6, nach dem die Tests (T5) einen Fehler
/// einspeisen. Produktivcode kennt nur [`ConversionStep::None`] — der Hook
/// wird von [`convert_plaintext_database`] mit einem Nichts-Tun übergeben,
/// damit genau derselbe Rumpf läuft, den T5 prüft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConversionStep {
    AfterEncryptedCopy,
    AfterVerification,
}

/// Der Referenzstand des Originals, gegen den Schritt 3 prüft.
#[derive(Debug, PartialEq, Eq)]
struct DatabaseFingerprint {
    user_version: i64,
    /// `(Tabellenname, Zeilenzahl)`, nach Namen sortiert.
    table_rows: Vec<(String, i64)>,
    /// `(version, checksum)` aus `_sqlx_migrations`, nach Version sortiert.
    migrations: Vec<(i64, Vec<u8>)>,
}

async fn fingerprint(conn: &mut SqliteConnection) -> Result<DatabaseFingerprint, sqlx::Error> {
    let user_version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;

    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' \
         AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&mut *conn)
    .await?;

    let mut table_rows = Vec::with_capacity(names.len());
    for name in names {
        // Tabellennamen kommen aus `sqlite_master` derselben Datei, nicht von
        // außen; sie lassen sich in SQLite ohnehin nicht als Parameter
        // binden. In doppelte Anführungszeichen gesetzt und darin verdoppelt,
        // damit auch ein Name mit Sonderzeichen nicht als Syntax gelesen wird.
        //
        // `AssertSqlSafe`: `sqlx` lehnt eine zur Laufzeit gebaute
        // SQL-Zeichenkette grundsätzlich ab und verlangt diese ausdrückliche
        // Zusicherung. Sie ist hier geprüft — der Name stammt aus
        // `sqlite_master` **derselben Datei**, nicht aus einer Eingabe, und
        // SQLite erlaubt für einen Tabellennamen keinen Parameter.
        let quoted = name.replace('"', "\"\"");
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM \"{quoted}\""
        )))
        .fetch_one(&mut *conn)
        .await?;
        table_rows.push((name, count));
    }

    let migrations: Vec<(i64, Vec<u8>)> =
        sqlx::query("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(|row| (row.get::<i64, _>(0), row.get::<Vec<u8>, _>(1)))
            .collect();

    Ok(DatabaseFingerprint {
        user_version,
        table_rows,
        migrations,
    })
}

/// A6: wandelt die Klartext-Datei unter `db_path` in eine mit `key`
/// verschlüsselte Datei um — in genau der Reihenfolge, die A6 vorgibt.
///
/// Nach `Ok(())` liegt unter `db_path` die verschlüsselte Datei und kein
/// Klartext-Original mehr. Nach `Err(_)` ist das Original **inhaltsgleich**
/// (sein WAL ist eingespielt, was den Inhalt nicht ändert), es liegt keine
/// Zwischendatei mehr, und es wurde nichts umbenannt. Migrationen laufen
/// danach wie gewohnt — nicht hier (Schritt 5).
pub async fn convert_plaintext_database(
    db_path: &Path,
    key: &DatabaseKey,
) -> Result<(), ConversionFailure> {
    convert_plaintext_database_inner(db_path, key, &mut |_| Ok(())).await
}

pub(crate) async fn convert_plaintext_database_inner(
    db_path: &Path,
    key: &DatabaseKey,
    after_step: &mut dyn FnMut(ConversionStep) -> Result<(), ConversionFailure>,
) -> Result<(), ConversionFailure> {
    // A6, letzter Satz / T20: Symlink vor jedem anderen Schritt prüfen —
    // bevor irgendetwas geöffnet oder angelegt ist.
    if std::fs::symlink_metadata(db_path)?.file_type().is_symlink() {
        return Err(ConversionFailure::Symlink);
    }

    // T19: Was an der Stelle der Zwischendatei liegt, wird verworfen —
    // vor dem Schreiben, nicht danach.
    discard_intermediate(db_path);

    let tmp = intermediate_path(db_path);
    let result = convert_steps(db_path, &tmp, key, after_step).await;
    if result.is_err() {
        // A6: „Scheitert ein Schritt vor 4: Original inhaltsgleich,
        // Zwischendatei entfernt."
        discard_intermediate(db_path);
    }
    result
}

async fn convert_steps(
    db_path: &Path,
    tmp: &Path,
    key: &DatabaseKey,
    after_step: &mut dyn FnMut(ConversionStep) -> Result<(), ConversionFailure>,
) -> Result<(), ConversionFailure> {
    // --- Schritt 1: Original öffnen, WAL vollständig einspielen, schließen.
    //
    // Ohne diesen Schritt bliebe der Inhalt des WAL in der alten Datei
    // liegen: `sqlcipher_export` liest den Datenbankzustand, der WAL wird
    // dabei zwar mitgelesen, aber Schritt 4 entfernt die alte `-wal` — und
    // eine alte `-wal` neben der neuen, verschlüsselten Datei wäre schlimmer
    // als ihr Verlust (sie gehört zu einer anderen Datei). Checkpoint
    // TRUNCATE spielt sie ein und leert sie.
    let expected = {
        let mut conn = open_plaintext(db_path, false)
            .await
            .map_err(ConversionFailure::Database)?;
        // spec-reviewer Runde 1: Das Ergebnis **auslesen**, nicht
        // wegwerfen. `wal_checkpoint` meldet "busy" als Ergebniszeile
        // (erste Spalte 1), nicht als Fehler. Lief der Checkpoint nicht
        // durch, ist die alte `-wal` nicht leer -- und Schritt 4 entfernt
        // sie trotzdem. Scheitert dann das `rename`, stünde das Original
        // ohne ein WAL da, das noch committete Frames enthielt. Lieber
        // hier sichtbar abbrechen, als "die -wal ist leer" anzunehmen.
        let checkpoint: (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(&mut conn)
            .await
            .map_err(ConversionFailure::Database)?;
        if checkpoint.0 != 0 {
            return Err(ConversionFailure::Verification(format!(
                "wal_checkpoint(TRUNCATE) meldete busy ({}) - das WAL des Originals ist \
                 nicht eingespielt",
                checkpoint.0
            )));
        }
        let fingerprint = fingerprint(&mut conn)
            .await
            .map_err(ConversionFailure::Database)?;
        conn.close().await.map_err(ConversionFailure::Database)?;
        fingerprint
    };

    // --- Schritt 2: verschlüsselte Kopie in die Zwischendatei schreiben.
    {
        let mut conn = open_plaintext(db_path, true)
            .await
            .map_err(ConversionFailure::Database)?;
        // Dateiname **und** Schlüssel als gebundene Parameter: so steht der
        // Schlüssel in keinem SQL-Text, den ein `sqlx`-Fehler zitieren
        // könnte (A2, Angriffsrichtung „SQL-Fehlertext mit dem PRAGMA-Wert").
        // `scrub_key_material` bleibt als zweite Schranke darüber.
        let pragma = key.pragma_value();
        sqlx::query("ATTACH DATABASE ? AS encrypted KEY ?")
            .bind(tmp.to_string_lossy().to_string())
            .bind(pragma.expose_secret().to_string())
            .execute(&mut conn)
            .await
            .map_err(|err| scrub_key_material(err, key))?;
        let export = sqlx::query("SELECT sqlcipher_export('encrypted')")
            .fetch_optional(&mut conn)
            .await
            .map_err(|err| scrub_key_material(err, key));
        // `DETACH` auch dann, wenn der Export gescheitert ist — sonst bliebe
        // die halbe Zwischendatei von dieser Verbindung offen und ließe sich
        // unter Windows nicht entfernen.
        let detach = sqlx::query("DETACH DATABASE encrypted")
            .execute(&mut conn)
            .await
            .map_err(|err| scrub_key_material(err, key));
        let close = conn.close().await.map_err(ConversionFailure::Database);
        export?;
        detach?;
        close?;
    }

    // §5: Die Zwischendatei trägt denselben Schutz wie die
    // Datenbank, und zwar **bevor** sie geprüft und umbenannt wird --
    // vorher entstand sie mit den umask-Rechten (typ. 0644) und enthielt
    // bereits die vollständige Kopie (spec-reviewer Runde 1).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let path = sibling(tmp, suffix);
            if path.exists() {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }

    // `user_version` und `journal_mode` überträgt der Export nicht (gemessen,
    // s. Spec 0101 §1) — auf der geöffneten Zwischendatei gesetzt, bleiben
    // beide erhalten.
    {
        let mut conn = open_encrypted(tmp, key)
            .await
            .map_err(|err| scrub_key_material(err, key))?;
        // `AssertSqlSafe`: `user_version` ist ein `i64` aus der Quelldatei,
        // kein Text; SQLite nimmt für ein PRAGMA keinen Parameter.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "PRAGMA user_version = {}",
            expected.user_version
        )))
        .execute(&mut conn)
        .await
        .map_err(ConversionFailure::Database)?;
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&mut conn)
            .await
            .map_err(ConversionFailure::Database)?;
        conn.close().await.map_err(ConversionFailure::Database)?;
    }

    after_step(ConversionStep::AfterEncryptedCopy)?;

    // --- Schritt 3: Zwischendatei neu öffnen und prüfen.
    {
        let mut conn = open_encrypted(tmp, key)
            .await
            .map_err(|err| scrub_key_material(err, key))?;

        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&mut conn)
            .await
            .map_err(ConversionFailure::Database)?;
        if integrity != "ok" {
            return Err(ConversionFailure::Verification(format!(
                "integrity_check ergab '{integrity}'"
            )));
        }

        let actual = fingerprint(&mut conn)
            .await
            .map_err(ConversionFailure::Database)?;
        if actual.table_rows != expected.table_rows {
            return Err(ConversionFailure::Verification(format!(
                "Zeilenzahlen je Tabelle weichen ab: erwartet {:?}, gefunden {:?}",
                expected.table_rows, actual.table_rows
            )));
        }
        if actual.migrations != expected.migrations {
            return Err(ConversionFailure::Verification(
                "_sqlx_migrations weicht ab".to_string(),
            ));
        }
        if actual.user_version != expected.user_version {
            return Err(ConversionFailure::Verification(format!(
                "user_version weicht ab: erwartet {}, gefunden {}",
                expected.user_version, actual.user_version
            )));
        }

        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut conn)
            .await
            .map_err(ConversionFailure::Database)?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            return Err(ConversionFailure::Verification(format!(
                "journal_mode ist '{journal_mode}' statt 'wal'"
            )));
        }

        conn.close().await.map_err(ConversionFailure::Database)?;
    }

    after_step(ConversionStep::AfterVerification)?;

    // --- Schritt 4: alte Journaldateien entfernen, dann atomar umbenennen.
    //
    // Die Reihenfolge ist wesentlich: Eine alte `-wal`/`-shm` neben der
    // **neuen**, verschlüsselten Datei gehört zu einer anderen Datenbank.
    // Würde SQLite sie beim nächsten Öffnen anwenden, wäre das Ergebnis
    // Datenmüll. Nach Schritt 1 ist die alte `-wal` eingespielt und leer,
    // ihr Verschwinden verliert also nichts. `-journal` (Rollback-Journal
    // eines Builds ohne WAL) aus demselben Grund mit.
    for suffix in ["-wal", "-shm", "-journal"] {
        let path = sibling(db_path, suffix);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(ConversionFailure::Io(err)),
        }
    }
    std::fs::rename(tmp, db_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(db_path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Öffnet eine **unverschlüsselte** Datei. Nur innerhalb der Umwandlung
/// benutzt.
///
/// `allow_create`: Schritt 1 öffnet mit `false`, damit ein Pfad ohne Datei
/// nicht still eine leere Datenbank anlegt, die danach als „umgewandelt"
/// gelten würde. Schritt 2 **muss** mit `true` öffnen — gemessen: `ATTACH`
/// erbt die Öffnungs-Flags der Hauptverbindung, und ohne
/// `SQLITE_OPEN_CREATE` scheitert das Anlegen der Zwischendatei mit
/// `code 14 „unable to open database"`. Dass das Original dann existiert,
/// ist zu diesem Zeitpunkt bereits bewiesen: Schritt 1 hat es geöffnet.
async fn open_plaintext(path: &Path, allow_create: bool) -> Result<SqliteConnection, sqlx::Error> {
    // spec-reviewer Runde 1: `cipher_log_level` und das Abschalten des
    // Statement-Logs gehören **auch** hierher. Diese Verbindung ist zwar
    // auf eine Klartext-Datei geöffnet, aber genau auf ihr läuft das
    // `ATTACH ... KEY ?` der Umwandlung -- sie arbeitet also mit
    // Schlüsselmaterial, und A8 gilt für sie wie für jede andere.
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(allow_create)
        .foreign_keys(true)
        .pragma("cipher_log_level", "NONE")
        .disable_statement_logging();
    SqliteConnection::connect_with(&options).await
}

/// Öffnet eine mit `key` verschlüsselte Datei. `create_if_missing(true)`:
/// Die Zwischendatei legt erst der `ATTACH` an, danach wird sie hier
/// geöffnet — und beim ersten Öffnen einer neuen Installation gibt es die
/// Datei noch nicht.
async fn open_encrypted(path: &Path, key: &DatabaseKey) -> Result<SqliteConnection, sqlx::Error> {
    SqliteConnection::connect_with(&encrypted_connect_options(path, key)).await
}

/// A2/A8: die Verbindungseinstellungen für eine verschlüsselte Datei.
///
/// `key` zuerst, dann `cipher_log_level` — SQLCipher nimmt den Schlüssel
/// nur als erste Anweisung auf einer frischen Verbindung an. `sqlx` führt
/// die hier eingetragenen Pragmas in Einfügereihenfolge aus und stellt
/// `key` seinen eigenen Standard-Pragmas voran; `tests_encryption`
/// (`test_a8_cipher_log_level_is_none`) prüft, dass beides tatsächlich
/// ankommt, statt es anzunehmen.
///
/// **Der Wert steht in doppelten Anführungszeichen** (`"x'…'"`), nicht nur
/// in der `x'…'`-Schreibweise. Gemessen: Ohne sie baut `sqlx` die Anweisung
/// `PRAGMA key = x'…'`, und daran scheitert SQLites Pragma-Grammatik mit
/// `code 1 „syntax error"` — ein Blob-Literal ist dort kein erlaubter Wert.
/// Mit den Anführungszeichen ist der Wert eine Zeichenkette, deren Inhalt
/// SQLCipher als rohen Schlüssel erkennt (dieselbe Schreibweise wie in
/// Spec 0101 §1 „Messungen": `"x'<64 Hex>'"`). Das Anführungszeichen selbst
/// kann im Wert nicht vorkommen — er besteht ausschließlich aus `x`,
/// Hex-Ziffern und zwei einfachen Anführungszeichen (s.
/// `DatabaseKey::pragma_value`).
///
/// `cipher_log_level = NONE` (A8): Ohne das schreibt SQLCipher auf Linux bei
/// einem falschen Schlüssel auf stderr. stderr wird nirgends ins Log
/// umgeleitet (Spec 0094), aber eine Fehlermeldung über Schlüsselmaterial
/// soll auch nicht in einem Terminal auftauchen, von dem aus die App
/// gestartet wurde.
pub(crate) fn encrypted_connect_options(path: &Path, key: &DatabaseKey) -> SqliteConnectOptions {
    let pragma = key.pragma_value();
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .pragma("key", format!("\"{}\"", pragma.expose_secret()))
        .pragma("cipher_log_level", "NONE")
        // **Das Wichtigste an diesen Optionen** (spec-reviewer Runde 1, A2
        // und §6 „Log/Redaction“): `sqlx` fasst alle Pragmas zu
        // **einer** Anweisung zusammen und führt sie durch seinen normalen
        // `QueryLogger`. Dessen Vorgaben sind `DEBUG` für jede Anweisung
        // und `WARN` für eine, die länger als eine Sekunde braucht --
        // also stand der vollständige Datenbankschlüssel mit
        // `RUST_LOG=debug` bei jedem Verbindungsaufbau und beim
        // Standard-Loglevel `info` auf dem Slow-Statement-Pfad in der
        // Logdatei. Die Redaktion auf dem Fehlerweg
        // (`redact_if_key_bearing`) greift dort nicht: Das ist kein Fehler,
        // sondern eine Erfolgsmeldung.
        //
        // `disable_statement_logging` schaltet beides ab. Dass es wirkt,
        // prüft `tests_encryption::
        // test_a2_the_database_key_never_appears_in_a_tracing_event` an
        // einem mitgeschnittenen `tracing`-Strom.
        .disable_statement_logging()
}
