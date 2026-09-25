//! Server-Profile nach `ssh_config` schreiben (Spec 0075, §3.2, §4.3).
//!
//! **Reine Logik, kein Dateisystem** — wie `plan::build_plan` auf der
//! Import-Seite (§4.1). Dieses Modul bekommt den Bestand und gibt fertigen
//! Dateitext samt einer Zusammenfassung zurück; wer den Dateidialog öffnet,
//! `~/.ssh/config` als Ziel ablehnt und tatsächlich schreibt, ist
//! `app-shell` — dieselbe Aufgabenteilung wie beim Import (§7.2).
//!
//! **Kein Geheimnis verlässt dieses Modul** (§3.2.2, §5.1 letzter Satz:
//! „Der Export schreibt keinen Wert, der aus dem Schlüsselbund stammt."):
//! Es liest ausschließlich [`Server`]-Felder, nie einen [`CredentialStore`]
//! — ein Passwort oder ein im Schlüsselbund liegender Schlüssel taucht in
//! den Eingaben dieses Moduls schlicht nicht auf.
//!
//! [`CredentialStore`]: crate::profiles::CredentialStore

use std::collections::{HashMap, HashSet};

use super::quoting::{quote_value, strip_control_chars};
use crate::profiles::types::{AuthMethod, Group, GroupId, PostIngestPolicy, Server};
use crate::shared::ServerId;

/// Ein Server, wie er tatsächlich exportiert wurde — Grundlage der
/// Abschlussmeldung (§3.2.7: „Die Abbildung wird gemeldet").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedServer {
    pub id: ServerId,
    /// `Server::name`, unverändert.
    pub original_name: String,
    /// Der tatsächlich geschriebene `Host`-Alias (§4.3).
    pub alias: String,
    /// `true` ⇒ `alias != original_name`, die Meldung MUSS das nennen.
    pub renamed: bool,
}

/// Ergebnis von [`build_export`] — der fertige Dateitext plus die
/// Grundlage für die Abschlussmeldung. Öffnet und schreibt keine Datei.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportPlan {
    pub text: String,
    /// In Exportreihenfolge — dieselbe wie im übergebenen `servers`-Slice,
    /// abzüglich des lokalen Pseudo-Servers (§3.2.4).
    pub exported: Vec<ExportedServer>,
}

/// §4.3: Alias-Regel. Zeichen außerhalb `[A-Za-z0-9._-]` werden zu `-`,
/// Mehrfach-`-` zusammengezogen, Ränder beschnitten.
fn sanitize_alias_base(name: &str) -> String {
    let substituted: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();

    let mut collapsed = String::with_capacity(substituted.len());
    let mut prev_dash = false;
    for c in substituted.chars() {
        if c == '-' {
            if !prev_dash {
                collapsed.push('-');
            }
            prev_dash = true;
        } else {
            collapsed.push(c);
            prev_dash = false;
        }
    }
    collapsed.trim_matches('-').to_string()
}

/// Vergibt Aliase für alle zu exportierenden Server, **vor** dem
/// eigentlichen Schreiben — nur so kann `ProxyJump` (§3.2.1) den
/// **umbenannten** Alias eines Ziels nennen, unabhängig von der
/// Reihenfolge im `servers`-Slice.
///
/// §4.3: Ist die bereinigte Form leer, wird `server-<n>` benutzt (`<n>` ein
/// Zähler über alle so entstandenen Fälle in diesem Export); eine
/// Kollision — mit einem echten Namen oder einem anderen `server-<n>` —
/// bekommt `-2`, `-3`, … angehängt. Die Spec legt die genaue Zählweise
/// nicht fest; diese ist deterministisch und für den Rundlauf ausreichend
/// (Design-Entscheidung, s. ADR 0075).
fn assign_aliases(servers: &[&Server]) -> HashMap<ServerId, String> {
    let mut used: HashSet<String> = HashSet::new();
    let mut next_empty: u32 = 1;
    let mut out = HashMap::with_capacity(servers.len());

    for s in servers {
        let base = sanitize_alias_base(&s.name);
        let base = if base.is_empty() {
            let name = format!("server-{next_empty}");
            next_empty += 1;
            name
        } else {
            base
        };
        let alias = if used.contains(&base) {
            let mut n = 2u32;
            loop {
                let candidate = format!("{base}-{n}");
                if !used.contains(&candidate) {
                    break candidate;
                }
                n += 1;
            }
        } else {
            base
        };
        used.insert(alias.clone());
        out.insert(s.id, alias);
    }
    out
}

/// §3.2.3-Kommentare tragen Werte, die **nicht** aus der `ssh_config`
/// selbst stammen, sondern aus dem Bestand — ein Gruppenname ist der Name
/// einer beim Import angelegten Datei (`file_stem_name`, roh übernommen),
/// ein Schlagwort kann wörtlich aus einer fremden Datei stammen (§3.1.3).
/// Beides landet hier in einer `#`-Kommentarzeile; ein `\n`/`\r` darin
/// würde die Zeile beenden und alles Folgende als **eigene, wirksame**
/// Direktive erscheinen lassen (spec-reviewer-Fund, Runde 1). Dieselbe
/// [`strip_control_chars`], die `quote_value` für Direktivenwerte benutzt —
/// eine Zeichenklasse, eine Funktion, nicht zwei, die auseinanderlaufen
/// könnten.
fn sanitize_comment_text(value: &str) -> String {
    strip_control_chars(value)
}

/// Kommentarzeilen über einem Block (§3.2.3): was für **diesen** Server
/// nicht abgebildet werden kann. Leer bleibt, was ohnehin leer/Vorgabe ist
/// — eine Zeile „hier nicht abgebildet" für etwas, das es gar nicht gibt,
/// wäre selbst irreführend.
fn block_comments(server: &Server, group_name: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();

    if let Some(g) = group_name {
        lines.push(format!(
            "# smart-ssh: Gruppe „{}“ ist hier nicht abgebildet.",
            sanitize_comment_text(g)
        ));
    }
    if !server.tags.is_empty() {
        let tags: Vec<String> = server
            .tags
            .iter()
            .map(|t| sanitize_comment_text(t))
            .collect();
        lines.push(format!(
            "# smart-ssh: Schlagworte ({}) sind hier nicht abgebildet.",
            tags.join(", ")
        ));
    }
    if !server.notes.trim().is_empty() {
        lines.push(
            "# smart-ssh: Es gibt Notizen zu diesem Server, hier nicht abgebildet.".to_string(),
        );
    }
    if server.post_ingest_policy != PostIngestPolicy::default() || server.ai_injection_check_enabled
    {
        lines.push(
            "# smart-ssh: abweichende Sicherheitseinstellungen (Nach-Lese-Eskalation/KI-Prüfung) sind hier nicht abgebildet.".to_string(),
        );
    }
    if let Some(path) = &server.sftp_server_path {
        lines.push(format!(
            "# smart-ssh: eigener sftp-server-Pfad ({}) ist hier nicht abgebildet.",
            sanitize_comment_text(path)
        ));
    }

    // §3.2.2/§3.2.3: die Art der Anmeldung — außer `IdentityFile`, die wird
    // als eigene Zeile geschrieben und braucht keinen Kommentar.
    match &server.auth {
        AuthMethod::IdentityFile { .. } => {}
        AuthMethod::Password { .. } => lines.push(
            "# smart-ssh: Anmeldung per Passwort — das Passwort wird nicht exportiert.".to_string(),
        ),
        AuthMethod::PrivateKey { .. } => lines.push(
            "# smart-ssh: Anmeldung per gespeichertem Schlüssel — der Schlüssel liegt im Schlüsselbund und wird nicht exportiert.".to_string(),
        ),
        AuthMethod::Certificate { .. } => lines.push(
            "# smart-ssh: Anmeldung per Zertifikat — Zertifikat und Schlüssel werden nicht exportiert.".to_string(),
        ),
        AuthMethod::Agent => lines.push(
            "# smart-ssh: Anmeldung über den SSH-Agent — ohne IdentityFile bietet ssh die vom Agent verwalteten Schlüssel automatisch an.".to_string(),
        ),
    }

    lines
}

fn group_name(groups: &[Group], id: Option<GroupId>) -> Option<&str> {
    let id = id?;
    groups.iter().find(|g| g.id == id).map(|g| g.name.as_str())
}

/// Baut den Exportplan (§3.2). `servers` bestimmt die Ausgabereihenfolge;
/// der lokale Pseudo-Server (`local_server_id`) wird übersprungen (§3.2.4,
/// §5.7 — die Unterscheidung kommt von außen, keine `if id == …` in der
/// Kernlogik dieses Moduls).
pub fn build_export(servers: &[Server], groups: &[Group], local_server_id: ServerId) -> ExportPlan {
    let exportable: Vec<&Server> = servers.iter().filter(|s| s.id != local_server_id).collect();
    let alias_of = assign_aliases(&exportable);

    let mut exported = Vec::with_capacity(exportable.len());
    let mut blocks = Vec::with_capacity(exportable.len());
    let mut rename_count = 0usize;

    for s in &exportable {
        let alias = alias_of
            .get(&s.id)
            .cloned()
            .unwrap_or_else(|| s.name.clone());
        let renamed = alias != s.name;
        if renamed {
            rename_count += 1;
        }
        exported.push(ExportedServer {
            id: s.id,
            original_name: s.name.clone(),
            alias: alias.clone(),
            renamed,
        });

        let mut block = String::new();
        for line in block_comments(s, group_name(groups, s.group_id)) {
            block.push_str(&line);
            block.push('\n');
        }
        block.push_str(&format!("Host {}\n", quote_value(&alias)));
        // §3.2.1 nennt `HostName` ohne Bedingung — die Spec setzt einen
        // nicht-leeren `host` voraus. §1.3 hält aber ausdrücklich fest,
        // dass das Anlegen von Hand keinen Pflichtfeld-Check kennt; ein
        // leerer `host` ist also erreichbar. `HostName ""` ist eine
        // fragwürdige Zeile (ob `ssh -G` sie akzeptiert, hängt von der
        // Version ab, s. ADR 0075) — ausgelassen verhält sich `ssh` wie
        // ohne `HostName`: es nimmt den Alias. Das ist die sicherere
        // Verallgemeinerung, keine Abweichung von einem Fall, den die Spec
        // tatsächlich bedacht hat.
        if !s.host.is_empty() {
            block.push_str(&format!("    HostName {}\n", quote_value(&s.host)));
        }
        // §3.2.1: nur wenn ≠ 22.
        if s.port != 22 {
            block.push_str(&format!("    Port {}\n", s.port));
        }
        // §3.2.1: nur wenn nicht leer.
        if !s.username.is_empty() {
            block.push_str(&format!("    User {}\n", quote_value(&s.username)));
        }
        // §3.2.1: `ProxyJump` nennt den (ggf. umbenannten) Alias des Ziels
        // (§3.2.1, Absatz „ProxyJump nennt den Alias, nicht den Namen").
        if let Some(jump_id) = s.jump_host {
            if let Some(jump_alias) = alias_of.get(&jump_id) {
                block.push_str(&format!("    ProxyJump {}\n", quote_value(jump_alias)));
            }
            // Zeigt `jump_host` auf ein Ziel, das hier nicht exportiert
            // wird (etwa den lokalen Pseudo-Server — der kann laut
            // `SERVER_JUMP_HOST_LOCAL` gar nicht als Jump-Host gesetzt
            // sein, s. `commands::reject_local_jump_host`), wird die Zeile
            // ausgelassen statt auf einen nicht existierenden Alias zu
            // zeigen. Defensiv: dieser Fall sollte laut Fremdschlüssel und
            // bestehender Regel nie eintreten.
        }
        // §3.2.1: nur bei `AuthMethod::IdentityFile` (Spec 0076).
        if let AuthMethod::IdentityFile { path, .. } = &s.auth {
            block.push_str(&format!("    IdentityFile {}\n", quote_value(path)));
        }
        blocks.push(block);
    }

    let mut text = String::new();
    text.push_str("# Smart SSH — Export nach ssh_config (Spec 0075)\n");
    text.push_str(&format!(
        "# {} Server exportiert{}.\n",
        exported.len(),
        if exported.is_empty() {
            ""
        } else {
            ", je Block unten"
        }
    ));
    text.push_str(
        "# Gruppen, Schlagworte, Notizen, Filterregeln, Risiko- und \n\
         # Sicherheitseinstellungen sowie die Art der Anmeldung haben in \n\
         # ssh_config kein Gegenstück. Was für einen einzelnen Server \n\
         # betroffen ist, steht als Kommentar über seinem Block.\n",
    );
    if rename_count > 0 {
        text.push_str(&format!(
            "# {rename_count} Servername(n) mussten für einen gültigen ssh_config-Alias umbenannt werden.\n"
        ));
    }
    text.push('\n');

    for block in &blocks {
        text.push_str(block);
        text.push('\n');
    }

    ExportPlan { text, exported }
}

#[cfg(test)]
mod tests;
