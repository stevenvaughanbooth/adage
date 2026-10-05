// The Day/Night choice, shared by every page: it follows the system until the visitor picks one,
// and the pick is remembered.

const KEY = "adage.theme";
const root = document.documentElement;
const mqLight = matchMedia("(prefers-color-scheme: light)");

export const isLight = () => (root.dataset.theme ? root.dataset.theme === "light" : mqLight.matches);

export function initTheme(onChange: (light: boolean) => void = () => {}) {
  try {
    const saved = localStorage.getItem(KEY);
    if (saved === "light" || saved === "dark") root.dataset.theme = saved;
  } catch { /* storage may be blocked; the system theme applies */ }
  const btn = document.getElementById("theme") as HTMLButtonElement | null;
  const sync = () => {
    const light = isLight();
    if (btn) {
      btn.setAttribute("aria-label", light ? "Switch to the night theme" : "Switch to the day theme");
      btn.textContent = light ? "Night" : "Day";
    }
    document.querySelectorAll<HTMLMetaElement>('meta[name="theme-color"]').forEach((m) => (m.content = light ? "#ececef" : "#050507"));
    onChange(light);
  };
  btn?.addEventListener("click", () => {
    root.dataset.theme = isLight() ? "dark" : "light";
    try { localStorage.setItem(KEY, root.dataset.theme); } catch { /* not remembered */ }
    sync();
  });
  mqLight.addEventListener("change", () => { if (!root.dataset.theme) sync(); });
  sync();
}
