// Test-only page for the narrow-layout smoke check (Issue #578): shows the
// hook's answer next to an element styled with the `narrow:` Tailwind variant.
import { createRoot } from "react-dom/client";
import "../../src/index.css";
import { useIsNarrow } from "../../src/hooks/useIsNarrow";

function Probe() {
  const narrow = useIsNarrow();
  return (
    <div>
      <p data-testid="hook">{narrow ? "narrow" : "wide"}</p>
      <p data-testid="css" className="hidden narrow:block">
        narrow
      </p>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<Probe />);
