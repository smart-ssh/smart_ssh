import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach } from "vitest";

// ADR 0122, R9: eine gemeinsame Obergrenze für asynchrones Warten
// (`waitFor`/`findBy*`) statt einzelner Fristen je Aufruf — großzügig genug,
// dass der gute Fall (reines Ereigniswarten, R6) nie an ihr scheitert, und
// deutlich unter `testTimeout` (`vite.config.ts`), damit ein Wackler als
// fehlgeschlagene Erwartung mit Inhalt erscheint statt als "Test timed out".
configure({ asyncUtilTimeout: 5000 });

afterEach(() => {
  cleanup();
});
