# ADR 0130 — Live-Ausgabe: Redaction auf stabilen Zeilen, Takt im Backend

Status: akzeptiert
Betrifft: Issue #325, Spec 0106, Spec 0006, Spec 0027, Spec 0043/0044

## Kontext

Spec 0106 zeigt die Ausgabe eines laufenden Kommandos live im Chat. Die
Live-Ausgabe darf nichts zeigen, was das redigierte Ergebnis verbirgt. Das
Issue schlug vor, auf ganzen Zeilen zu redigieren. Der Redactor hat aber
Muster, die über Zeilen reichen: Private-Key-Blöcke (`(?s)BEGIN … END`, ohne
Ende bis zum Textende), Werte in Anführungszeichen mit einem Zeilenumbruch
(`sshpass -p 'a⏎b'`), `password:` mit dem Wert auf der Folgezeile (`\s*`
über Zeilenumbrüche), `.netrc`-Einträge über mehrere Zeilen. Zeilenweise
Redaction zeigte in allen diesen Fällen Teile, die das Ergebnis maskiert
(nachgewiesen: die Tests in `core::ai::live_redaction` schlagen mit
zeilenweiser Redaction fehl).

## Entscheidung

1. **Transport:** `SshTransport::execute_streaming(command, stdin, cancel,
   sink)` mit Default-Rumpf (delegiert ohne Live-Ausgabe an die bisherigen
   abbrechbaren Methoden), damit bestehende Implementierungen und Mocks
   unverändert bleiben. `RusshTransport` und `LocalTransport` reichen jeden
   behaltenen Chunk (nach dem Cap, ohne den Kürzungstext) und einmal
   `Truncated` an einen `mpsc`-Kanal weiter. Der Cap-Kern (`CappedOutput`)
   ist die einzige Stelle, an der das passiert. Das Ergebnis ist
   unverändert, Fehler beim Senden werden ignoriert (nur Anzeige).
2. **Redaction (`core::ai::LiveOutputRedactor`, je Strom):** Freigabe nur
   ganzer Zeilen (`\n`, `\r\n`, einzelnes `\r`). Freigegeben werden die
   Zeilen bis zu einer Schnittstelle, wenn
   - danach mindestens eine weitere ganze Zeile vorliegt (Vorausschau),
   - die Redaction von *Kontext + Zeilen* mit einem Zeilenende endet (ein
     Treffer schluckt das Zeilenende nicht, z. B. offener Schlüsselblock),
   - sie ein Präfix der Redaction von *Kontext + allem Empfangenen* ist
     (kein Treffer reicht über die Schnittstelle hinaus).
   Kontext sind bis zu 4 KB bereits freigegebener Rohtext, damit ein Wert,
   dessen Schlüssel vorher stand, erkannt wird. Ändert sich die Redaction
   des Kontexts selbst, wird statt der neuen Zeilen ein Platzhalter
   gezeigt. Geprüft werden höchstens drei Schnittstellen je Freigabe; mehr
   als 256 KB ohne stabile Stelle stoppt die Live-Anzeige des Stroms.
3. **Takt im Backend:** `forward_live_output` sammelt Chunks und gibt sie
   höchstens alle 100 ms als ein `chat-action-output`-Event je Aktion frei
   (`tokio::time::interval`, `MissedTickBehavior::Delay`). Der Takt liegt im
   Backend, damit auch die Redaction-Arbeit gedrosselt ist.
4. **Kein Abschluss-Flush:** Was beim Ende noch zurückgehalten ist, wird
   nicht live gesendet; das Ergebnis ersetzt die Anzeige ohnehin.
5. **Frontend:** Live-Zustand außerhalb der Chat-Elemente, damit die
   Aktualisierungen den Chat nicht ans Ende springen lassen. Angezeigt wird
   nur das jüngste Stück (20 000 Zeichen je Strom).

## Abwägung und Einschränkung

- **Vorausschau kostet Unmittelbarkeit.** Die letzte Zeile eines Stroms
  erscheint erst mit der nächsten oder mit dem Ergebnis. Bei `for i in 1 2 3
  4 5; do echo $i; sleep 1; done` erscheinen 1–4 live im Sekundentakt, die 5
  mit dem Ergebnis. Ein Freigeben nach Zeitablauf hätte die letzte Zeile
  sofort gezeigt, aber genau den Fall „Wert in Anführungszeichen, Rest auf
  der nächsten Zeile" offengelegt. Sicherheit geht vor.
- **Grenzen der Vorausschau.** Muster, die Text *vor* einem Geheimnis erst
  durch mehr als eine spätere Zeile maskieren (z. B. Rechnername und Login
  eines `.netrc`-Eintrags, dessen Passwort zwei Zeilen später steht), können
  diesen Kontext live zeigen; das Geheimnis selbst bleibt maskiert, weil es
  mit dem davorliegenden Kontext redigiert wird. Ein Kontext über 4 KB
  hinaus wird nicht betrachtet.
- **Localhost bleibt nicht abbrechbar** (Spec 0027): `execute_streaming`
  ignoriert dort `cancel` wie die bisherigen Default-Methoden.

## Konsequenzen

- Eine neue Trait-Methode mit Default; andere Implementierungen von
  `SshTransport` kompilieren unverändert und zeigen dann keine
  Live-Ausgabe, bis sie die Methode überschreiben.
- Neue Redactor-Muster brauchen keine Anpassung hier; die Stabilitätsprüfung
  arbeitet mit jedem Muster, solange ein Treffer nicht mehr als eine
  folgende Zeile braucht.
