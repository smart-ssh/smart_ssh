# Spec: Server-/Gruppen-Kennzeichnung bei Notiz-Vorschlägen

Status: Entwurf
Modul: `frontend/` (Bestätigungsdialog, Disconnect-Benachrichtigung)
Abhängigkeiten: Notiz-Vorschlag beim Beenden (Spec 0010), Änderungs-Vorschau
(Spec 0019), Multi-Tab-Sessions (Spec 0017), Bugfix Ziel-Auflösung (Spec
0016, Abschnitt 6)

## 1. Problem

Ein Notiz-Vorschlag für Server A wurde angezeigt, während der Nutzer gerade
Server B geöffnet hatte — ohne erkennbaren Hinweis, dass sich der Vorschlag
auf einen anderen Server bezieht. Das Backend hat korrekt gehandelt (der
Vorschlag landete auch tatsächlich bei Server A, nicht bei B, gemäß der
serverseitigen Zielauflösung aus Spec 0016, Abschnitt 6) — das Problem ist
rein die fehlende Kennzeichnung in der Anzeige. Das widerspricht dem
Kernprinzip der App: Der Nutzer muss immer eindeutig erkennen können, worauf
sich eine Bestätigung bezieht, unabhängig davon, was er gerade auf dem
Bildschirm hat.

Ursache vermutlich: Die Disconnect-Benachrichtigung (`NoteSuggestionToast`,
Spec 0010/0019) wurde als bewusst tab-/kontext-unabhängige Benachrichtigung
gebaut (Spec 0010, Abschnitt 2, Punkt 6: "auch dann noch... wenn der Nutzer
inzwischen zu einem anderen Screen navigiert hat") — dabei wurde
offensichtlich vergessen, den Servernamen mit anzuzeigen, weil zum
Zeitpunkt der Implementierung meist nur ein Server offen war und der Bezug
"zufällig" klar schien.

## 2. Ziel

Jede Darstellung eines Notiz-Vorschlags — egal ob als reguläre
Chat-Aktionskarte oder als Disconnect-Benachrichtigung, egal ob der
betroffene Server/die Gruppe gerade sichtbar ist oder nicht — zeigt
**immer und deutlich sichtbar** den Namen des Ziels (Server- oder
Gruppenname) an, nicht nur den Notizinhalt selbst.

## 3. Umsetzung

- `chat-action-proposed` und `note-update-suggested` (Spec 0019, Abschnitt
  3) enthalten bereits ein aufgelöstes Ziel serverseitig — ergänze das
  Event-Payload um `targetName: string` (Server- oder Gruppenname, bereits
  zum Zeitpunkt der Zielauflösung bekannt, kein zusätzlicher Command nötig).
- **`NoteSuggestionToast`**: zeigt `targetName` prominent im Titel/Header
  der Benachrichtigung (z. B. "Notiz-Vorschlag für Server 'Proxmox'"), nicht
  nur als Nebeninfo im Fließtext.
- **Aktionskarte im Chat** (`ChatPanel`, regulärer In-Chat-Vorschlag): zeigt
  `targetName` ebenfalls, **auch wenn** es sich um den aktuell offenen
  Server der jeweiligen Session handelt — Konsistenz ist hier wichtiger als
  Redundanz zu vermeiden, und verhindert eine Klasse von Bugs wie den
  gemeldeten, falls sich der Anzeigekontext künftig ändert (z. B. durch
  Multi-Tab, Spec 0017).
- Bezieht sich der Vorschlag auf eine **Gruppe** statt einen Server
  (`NoteTarget::Group`, Spec 0003), wird zusätzlich zum Gruppennamen ein
  klar erkennbares Label ("Gruppen-Notiz", nicht "Server-Notiz") angezeigt —
  sonst entsteht dieselbe Verwechslungsgefahr auf einer zweiten Achse
  (Server vs. Gruppe statt nur Server A vs. Server B).

## 4. Test

Regressionstest für genau den gemeldeten Fall: Ein `ProposeNoteUpdate` für
Server A wird ausgelöst, während im Frontend-State Server B als "aktuell
betrachtet" markiert ist — das gerenderte Event zeigt nachweislich den
Namen von Server A, nicht B und nicht gar keinen Namen.

## 5. Offene Punkte

- Keine — dies ist ein reiner Anzeige-Bugfix ohne neue Design-Entscheidungen.
