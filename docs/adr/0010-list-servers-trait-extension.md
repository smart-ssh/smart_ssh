# 0010-list-servers-trait-extension

## Status
Accepted

## Kontext

`docs/specs/0007-tauri-app-mvp.md`, Abschnitt 4, verlangt für Teil 1 einen
Tauri-Befehl `list_servers() -> Vec<ServerDto>` für die einfache
Serverliste im MVP-Screen (Abschnitt 7: "keine Anlege-/Bearbeiten-UI",
nur Anzeige).

Der `ProfileStore`-Trait aus `core::profiles` (Spec 0003, Abschnitt 6;
Spec 0004) kennt dafür jedoch keine passende Methode — er bietet nur
gezielte Einzel-Lookups (`get_server(id)`, `get_group(id)`) sowie
schreibende Operationen, aber keine "alle Server auflisten"-Abfrage. Diese
Lücke war beim Entwurf von Spec 0003/0004 nicht sichtbar, weil dort noch
keine UI existierte, die eine vollständige Liste ohne bekannte IDs
brauchte.

## Entscheidung

`ProfileStore` bekommt eine zusätzliche Methode:

```rust
#[async_trait]
pub trait ProfileStore: Send + Sync {
    async fn list_servers(&self) -> ProfileResult<Vec<Server>>;
    // ... bestehende Methoden unverändert
}
```

Implementiert in allen vier bestehenden `ProfileStore`-Implementierungen:
`SqliteProfileStore` (`SELECT ... FROM servers ORDER BY name`, inkl.
`server_tags`-Join wie bei `get_server`), sowie den drei
In-Memory-Testdoubles (`DemoProfileStore` im `profiles_demo`-Beispiel,
`MockProfileStore` in `core::ssh::tests`, `InMemoryProfileStore` in
`core::profiles::tests`) — dort jeweils ein triviales
`self.servers.lock().unwrap().values().cloned().collect()`.

Kein separater `AiProviderStore`-artiger Sonderweg (z. B. eine
`app-tauri`-lokale Methode, die direkt auf `SqliteProfileStore`-Interna
zugreift): `list_servers` ist eine genuine `ProfileStore`-Fähigkeit
("alle Server", nicht "alle AI-Provider-Configs" wie bei
`SqliteAiProviderStore`, s. dessen Modul-Kommentar), gehört also in den
bestehenden Trait statt an ihm vorbei.

## Konsequenzen

**Positiv:**
- `app-tauri`s `list_servers`-Befehl bleibt ein dünner Wrapper (Spec 0007,
  Abschnitt 3: "keine fachliche Logik") — er ruft nur
  `profile_store.list_servers()` auf, statt eine eigene Query gegen
  `SqliteProfileStore`-Interna zu bauen und damit die
  I/O-Kapselung von `core` zu unterlaufen.
- Zukünftige `ProfileStore`-Implementierungen (falls je ein zweites
  Backend als SQLite entsteht) bekommen die Methode über den Trait
  erzwungen, statt dass sie leicht vergessen werden könnte.

**Negativ / Trade-off:**
- Weicht von Spec 0003/0004 ab, die `ProfileStore` ursprünglich ohne
  Auflistungs-Methode definiert haben — wer nur diese beiden Specs liest,
  kennt `list_servers` nicht.
- Vier statt eine Implementierung mussten angepasst werden (drei davon
  nur Testdoubles) — bei einer größeren Zahl von `ProfileStore`-Mocks
  wäre das ein zunehmend spürbarer Wartungsaufwand bei jeder künftigen
  Trait-Erweiterung.
