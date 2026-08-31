# 0005-credential-store-in-profiles

## Status
Accepted

## Kontext

`docs/specs/0001-architecture-overview.md`, Abschnitt 3, sieht in der
Architektur-Übersicht ein eigenes Top-Level-Modul `crates/core/credentials/`
für die "Credential-Store-Abstraktion" vor. Dieses Modul existierte bereits
als leeres Skelett (aus dem initialen Workspace-Grundgerüst) mit dem
Kommentar-Inhalt "Sichere Ablage von Zugangsdaten", "Zuordnung Credential
<-> Server/Projekt" etc. — inhaltlich genau das, was `CredentialRef` und der
`CredentialStore`-Trait tun.

`docs/specs/0003-server-profile-datenmodell.md`, Abschnitt 4, definiert
`CredentialRef` und `CredentialStore` jedoch als Teil des
Server-Profil-Datenmodells, direkt neben `Server`/`AuthMethod` — und der
konkrete Implementierungsauftrag für diesen Schritt verlangte explizit,
Abschnitt 2–4 der Spec (inklusive `CredentialStore`) in
`crates/core/src/profiles/` umzusetzen.

Damit beanspruchen zwei Module denselben fachlichen Bereich: das leere
`core::credentials` (laut Architektur-Spec vorgesehener Ort) und das neue
`core::profiles` (laut Implementierungsauftrag tatsächlicher Ort).

## Entscheidung

`CredentialRef` und der `CredentialStore`-Trait (inkl. `CredentialError`)
werden vorerst in `crate::profiles` implementiert, nicht in
`crate::credentials`. Das leere `credentials`-Modul bleibt unverändert als
Platzhalter bestehen, bekommt aber einen Kommentar, der auf diese
Entscheidung verweist, damit niemand versehentlich eine zweite,
konkurrierende `CredentialRef`/`CredentialStore`-Definition dort anlegt.

Begründung: `CredentialRef` ist ein Feld von `AuthMethod`, das wiederum ein
Feld von `Server` ist — die drei Typen sind ohne einander nicht sinnvoll
konstruierbar bzw. testbar. Sie in zwei verschiedene Module (`profiles` für
`Server`/`AuthMethod`, `credentials` für `CredentialRef`/`CredentialStore`)
aufzuteilen, hätte für diesen Schritt nur eine Cross-Modul-Abhängigkeit ohne
fachlichen Gewinn erzeugt (`profiles` hätte sofort von `credentials`
abhängen müssen). `core::credentials` bleibt der vorgesehene Ort für die
**konkrete**, OS-Keychain-gestützte Implementierung des Traits (via
`keyring`-Crate, Spec 0001 Abschnitt 2) — reine Trait-/Typ-Definitionen sind
davon unabhängig und können vorerst dort bleiben, wo sie fachlich zuerst
gebraucht werden.

## Konsequenzen

**Positiv:**
- Keine verfrühte Modul-Aufteilung ohne konkreten Grund — `Server`,
  `AuthMethod`, `CredentialRef` und `CredentialStore` sind in diesem Schritt
  zusammen entstanden und bleiben zusammen, testbar mit einer einzigen
  `InMemoryCredentialStore`/`InMemoryProfileStore`-Testsuite.
- Der Kommentar in `credentials/mod.rs` verhindert stille Divergenz (zwei
  `CredentialRef`-Typen an verschiedenen Stellen).

**Negativ / Trade-off:**
- Weicht von der in Spec 0001 skizzierten Modul-Aufteilung ab, ohne dass
  Spec 0001 dafür formell angepasst wurde — wer nur Spec 0001 liest, sucht
  `CredentialStore` am falschen Ort.
- Sobald die konkrete Keychain-Implementierung ansteht, muss entschieden
  werden, ob `CredentialRef`/`CredentialStore` nach `credentials`
  **verschoben** und aus `profiles` nur re-exportiert werden, oder ob
  `credentials` von vornherein nur die konkrete Implementierung enthält und
  aus `profiles` importiert. Diese ADR trifft diese Folgeentscheidung
  bewusst noch nicht — sie hält nur fest, warum der aktuelle Zwischenzustand
  (beide Typen in `profiles`) kein Versehen ist.
