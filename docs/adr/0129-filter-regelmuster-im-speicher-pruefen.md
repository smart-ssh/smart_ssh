# ADR 0129 — Regelmuster im Regelspeicher prüfen, ungültige Regel einmal melden

Status: akzeptiert
Betrifft: Issue #260, Spec 0077 (3.1.2, 3.2.2), ADR 0068 (Abschnitt 7)

## Entscheidung

1. `SqlitePolicyStore::create`/`update` prüfen `Pattern::validate()` selbst
   und liefern `PolicyStoreError::InvalidPattern`. Die Prüfung in
   `app-logic` bleibt (Schichtung). Tests, die eine Regel „aus einer
   älteren Datenbank" brauchen, nutzen `create_unchecked_for_tests`
   (hinter `test-support`).
2. `RuleWriteError` implementiert weder `Display` noch `Error`. Dadurch
   greift der pauschale `From<E: Display> for CommandError` nicht, und ein
   eigener `From<RuleWriteError> for CommandError` setzt den Code. `?`
   verliert ihn nicht mehr. Die Funktion `rule_write_error` entfällt.
3. Die Meldung aus Spec 0077 3.2.2 wird je Regel-Kennung und
   Muster-Hash einmal ausgegeben. Der Zustand liegt **in der
   `FilterEngine`**, ist auf 1024 Einträge begrenzt (bei Überlauf geleert)
   und hält nur den Hash, nie den Mustertext.

## Abwägung: je Engine statt je Prozess

Der Auftrag nannte „einmal je Prozesslauf". Ein prozessweiter Zustand
(`static`) würde die bestehenden Spec-0077-Tests stören, die dieselbe
Regel-Kennung mit demselben Muster in parallelen Tests auswerten, und sie
müssten geändert werden. Eine Engine lebt je Verbindung; die Meldung
erscheint also einmal je Verbindung und Regel statt je Auswertung. Das
beseitigt die Flut; ein prozessweiter Zustand ließe sich später ohne
Verhaltensänderung nachziehen.
