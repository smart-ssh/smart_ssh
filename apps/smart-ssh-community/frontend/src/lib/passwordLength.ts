/** Spec 0101, A13: Zeichen zählen wie das Backend.
 *
 * `String.length` zählt UTF-16-Einheiten: Sechs Emoji ergeben dort 12 und
 * im Backend (`chars().count()`, `crypto::key_wrapping`) sechs — der Knopf
 * wäre scharf, und die Antwort lautete „mindestens 12 Zeichen". Die
 * Oberfläche darf nicht das Gegenteil dessen behaupten, was das Backend
 * gleich sagt (A20). Maßgeblich bleibt das Backend; dies ist die
 * freundliche Hälfte (Review-Fund Runde 1 zu Commit 11).
 *
 * `[...value]` iteriert über Codepunkte, genau wie Rusts `chars()`.
 * Kombinierende Zeichen zählen in beiden Welten einzeln — hier geht es um
 * Gleichstand mit dem Backend, nicht um eine bessere Zählung.
 */
export function characterCount(value: string): number {
  return [...value].length;
}
