# Mehrere Coder parallel

## Vor dem Start

1. **Spec auf `main` committen.** Worktrees zweigen vom Default-Branch ab —
   eine uncommittete Spec sieht der Coder nicht.
2. **Spec- und ADR-Nummer im Auftrag vergeben.** Sonst wählen zwei Coder
   dieselbe "nächste freie Nummer".
3. **Aufträge mit wenig Überschneidung parallel laufen lassen.** Zwei Coder
   im selben Modul (z. B. beide in `commands.rs` oder im Dateibrowser)
   erzeugen Merge-Konflikte — dann lieber nacheinander.

## Starten

**Variante A — eigene Sitzung pro Coder (Rückfragen möglich):**

```bash
claude --worktree spec-0069 --agent smart-ssh-coder
```

Der Worktree landet unter `.claude/worktrees/spec-0069/` auf dem Branch
`worktree-spec-0069`. Den Auftrag (Prompt) dann in dieser Sitzung einfügen.
Für einen zweiten Coder ein zweites Terminal mit anderem Namen.

**Variante B — Subagent aus einer koordinierenden Sitzung:**
"Setze Spec 0069 mit dem smart-ssh-coder-Agent in einem eigenen Worktree
um" — läuft im Hintergrund in einem temporären Worktree, kann aber nicht
zurückfragen: Er hält nach Teil 0 an und berichtet; die Fortsetzung schickst
du als Nachricht. Den eigenen Worktree ausdrücklich verlangen: Der Agent
legt ihn nicht selbst an, weil er auch in bereits vorbereiteten Worktrees
gestartet wird.

## Zusammenführen

```bash
git checkout main
git merge --no-ff worktree-spec-0069
```

Danach die Changelog-Fragmente beim nächsten Release einsammeln
(`changelog.d/README.md`). Worktree aufräumen:

```bash
git worktree list
git worktree remove .claude/worktrees/spec-0069
```

## Bekannte Stolpersteine

- **Build-Zeit und Speicher:** jeder Worktree hat sein eigenes
  `target/`-Verzeichnis — der erste Rust-Build ist jeweils kalt und belegt
  mehrere GB. Ein gemeinsames `CARGO_TARGET_DIR` ist bei parallelen Builds
  keine gute Idee (Sperren, gegenseitiges Invalidieren); `sccache` hilft.
- **Frontend:** jeder Worktree braucht ein eigenes `npm ci`.
- **Nicht versionierte Dateien** (z. B. lokale `.env`) fehlen im Worktree —
  per `.worktreeinclude` gezielt mitkopieren lassen.
- **Dev-App:** nur im Haupt-Checkout nach dem Merge starten; parallele
  Dev-Apps kollidieren bei Port und Datenverzeichnis.
