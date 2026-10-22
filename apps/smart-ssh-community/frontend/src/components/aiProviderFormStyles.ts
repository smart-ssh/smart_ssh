import { FIELD_CONTROL_CLASS } from "./ui";

/** Spec 0056, Teil 3: gemeinsame Design-Tokens für dieses Formular statt
 * pro Feld wiederholter Ad-hoc-Klassenketten — alle vier Farbpaletten
 * (`slate`/`indigo`/`emerald`/`red`) sind bereits projektweite Tokens
 * (`index.css`s `@theme`), hier nur konsistent auf Formularfelder/
 * Sekundäraktionen angewandt. `bg-slate-950` für Felder INNERHALB einer
 * `bg-slate-900/40`-Karte erzeugt die vom Reviewer gewünschte
 * Kontraststufung (Modal-Grund `slate-800` → Karte `slate-900/40` → Feld
 * `slate-950`), `focus:ring-indigo-500` ist der bislang fehlende sichtbare
 * Fokus-Zustand.
 *
 * Spec-Reviewer-Fund (Spec 0056, Review dieses Schritts): `FIELD_CLASS`
 * enthielt ursprünglich auch Layout-Klassen (`mt-1 w-full`) — an drei
 * Stellen per `` `${FIELD_CLASS} mt-0 w-1/2` `` überschrieben. Tailwind
 * löst widersprüchliche Utility-Klassen über die Reihenfolge im
 * generierten Stylesheet, NICHT über die Reihenfolge im `className`-String
 * — die Overrides griffen deshalb nie (`mt-1`/`w-full` gewannen immer),
 * mit sichtbarem Versatz beim Modell-Feld/den Zusatz-Header-Feldern. Layout
 * (Abstand/Breite) ist jetzt bewusst NICHT Teil von `FIELD_CLASS`, sondern
 * wird an jeder Verwendungsstelle explizit gesetzt — ein echter Konflikt
 * ist damit strukturell ausgeschlossen. Ebenso `LABEL_CLASS`: `font-medium`
 * lag vorher auf dem `<label>` selbst und vererbte sich dadurch auf die
 * darin verschachtelten `<input>`/`<select>` (Tailwind-Preflight setzt dort
 * `font: inherit`) — Nutzereingaben erschienen halbfett. Der Text-Stil
 * liegt jetzt auf einem separaten `<span>` um den Label-Text, nicht mehr
 * auf dem Label-Element. */
export const CARD_CLASS = "rounded-lg border border-slate-700 bg-slate-900/40 p-4";
export const LABEL_CLASS = "block text-sm";
export const LABEL_TEXT_CLASS = "font-medium text-slate-200";
export const FIELD_CLASS = FIELD_CONTROL_CLASS;
