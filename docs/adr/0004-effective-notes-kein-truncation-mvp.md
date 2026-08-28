# 0004-effective-notes-kein-truncation-mvp

## Status
Accepted

## Kontext

`docs/specs/0003-server-profile-datenmodell.md`, Abschnitt 6, listet als
offenen Punkt:

> Maximale Länge/Token-Budget für `effective_notes()` — bei tiefen
> Gruppenhierarchien mit langen Notizen muss ggf. gekürzt oder priorisiert
> werden, bevor es an die KI-API geht (Kosten- und Kontextlimit-Frage,
> gehört eng mit der KI-Provider-Spec zusammen).

Beim Implementieren von `effective_notes()`
(`crates/core/src/profiles/notes.rs`) musste diese Frage tatsächlich
beantwortet werden: die Funktion sammelt Notizen von der Wurzel-Gruppe bis
zum Server und verkettet sie zu einem String — ohne eine Entscheidung, ob
und wie gekürzt wird, wäre stillschweigend "kein Limit" implementiert
worden, statt das bewusst zu entscheiden.

## Entscheidung

`effective_notes()` kürzt, priorisiert oder limitiert **nichts**. Sie gibt
den vollständigen, zusammengesetzten Text zurück, unabhängig davon, wie tief
die Gruppenhierarchie ist oder wie lang die einzelnen `notes`-Felder sind.

Begründung: ein sinnvolles Limit ist nicht MVP-fähig zu bestimmen, weil es
vom tatsächlich verwendeten KI-Modell abhängt (Kontextfenster-Größe,
Preis pro Token, evtl. modellspezifisches Tokenizer-Verhalten) — Wissen, das
laut Projektarchitektur (Spec 0001) beim `ai`-Modul bzw. der noch nicht
existierenden KI-Provider-Spec liegt, nicht bei `profiles`. `profiles` kennt
das Zielmodell einer Session nicht und soll es auch nicht kennen müssen
(sonst bräuchte `effective_notes()` einen Provider/Modell-Parameter, was die
Funktion an ein Detail koppeln würde, das mit LLM-Kontextnotizen konzeptionell
nichts zu tun hat).

## Konsequenzen

**Positiv:**
- `effective_notes()` bleibt eine reine, einfache Funktion ohne Kenntnis von
  KI-Providern, Modellen oder Preisen — genau der in Spec 0001 Abschnitt 3
  geforderten UI-/Provider-Unabhängigkeit von `core` entsprechend.
- Keine verlorene Information: Kürzung an dieser Stelle würde
  unwiederbringlich Kontext wegschneiden, den ein Aufrufer mit einem
  größeren Kontextfenster durchaus hätte nutzen können.

**Negativ / Trade-off:**
- Bei sehr tiefen Gruppenhierarchien mit langen Notizen kann der
  zurückgegebene String beliebig groß werden — ein naiver Aufrufer, der ihn
  ungeprüft an eine KI-API weiterreicht, kann dadurch überraschend hohe
  Kosten verursachen oder an ein Kontextlimit stoßen.
- Diese Entscheidung verschiebt das Problem lediglich, löst es nicht: die
  künftige KI-Provider-Spec **muss** eine Kürzungs-/Priorisierungsstrategie
  definieren (z. B. spezifischste Gruppen zuerst kürzen, da laut Spec 0003
  Abschnitt 5.1 "spätere/spezifischere Einträge mehr Gewicht" haben), bevor
  `effective_notes()`-Ergebnisse produktiv an ein LLM gehen. Diese ADR dient
  auch als Merkposten dafür.
