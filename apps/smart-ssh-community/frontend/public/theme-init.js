// Issue #804: runs synchronously from <head>, before the first paint, so there
// is no dark -> light flash. Mirrors `initTheme` in src/theme.ts (cache key and
// resolution must stay identical). External file because the CSP forbids
// inline scripts.
(function () {
  var choice = "system";
  try {
    var v = window.localStorage.getItem("smart-ssh.theme");
    if (v === "light" || v === "dark" || v === "system") choice = v;
  } catch (e) {}
  var dark = true;
  if (choice === "light") dark = false;
  else if (choice === "system" && typeof window.matchMedia === "function") {
    dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  }
  var theme = dark ? "dark" : "light";
  document.documentElement.setAttribute("data-theme", theme);
  document.documentElement.style.colorScheme = theme;
})();
