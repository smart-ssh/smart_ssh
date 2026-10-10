# ADR 0130 — Abbruch lokaler Kommandos über Prozessgruppe bzw. Prozessbaum

Status: akzeptiert
Betrifft: Issue #324, Spec 0027 (3.4), Spec 0032, Spec 0044

## Entscheidung

1. Jedes Einzelkommando des lokalen Pseudo-Servers läuft unter Unix in einer
   eigenen Prozessgruppe (`process_group(0)`), auch ohne Abbruch. Nur so
   erreicht ein Abbruch Pipelines, Subshells und Hintergrundprozesse.
2. Ablauf beim Abbruch (Unix): `SIGINT` an die Gruppe, höchstens 500 ms
   weiterlesen (Ausgabe unter demselben Cap), dann `SIGKILL` an die Gruppe,
   **danach** erst den Kindprozess einsammeln. Bis dahin hält der
   Gruppenleiter als Zombie die Gruppen-ID belegt, `killpg` kann also keine
   fremde, neu vergebene Gruppe treffen. `SIGKILL` geht immer an die Gruppe,
   auch wenn der Leiter schon beendet ist, weil Hintergrundprozesse einer
   nicht-interaktiven Shell `SIGINT` ignorieren.
3. Windows: `taskkill /T /F /PID <pid>` statt eines Job Objects.
4. Für `killpg` wird `libc` (Unix-only) direkte Abhängigkeit von
   `ssh-transport`. Die Kiste ist bereits direkte Abhängigkeit von
   `app-logic` (ADR 0065) und im Lockfile; keine neue Kiste.
5. Nur ein tatsächlich gesendeter Abbruch zählt. Ein ohne Senden gedroppter
   Sender bricht nicht ab (der SSH-Pfad wertet ihn als Abbruch; der
   Aufrufer droppt ihn während der Ausführung aber nie).
6. Der Cap-Abbruch (Spec 0044) bleibt unverändert beim Beenden nur des
   direkten Kindprozesses; der Auftrag betraf nur den Nutzer-Abbruch.

## Abwägung

Ein Job Object würde unter Windows auch Prozesse erfassen, die sich vom
Elternprozess lösen, braucht aber `windows-sys` als neue direkte
Abhängigkeit und `unsafe` WinAPI-Aufrufe. `taskkill /T` ist Teil jedes
Windows, beendet den Baum ab `cmd.exe` und reicht für die Zusage aus Spec
0027 („so nah, wie die Plattform es erlaubt"). Ein Prozess, der sich
ausdrücklich löst (`setsid`, `start` unter Windows), wird auf keiner
Plattform erreicht; Spec 0027 nennt das als Grenze.
