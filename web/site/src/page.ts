import { initScenes } from "./scenes";

// What every page shares below its header: sections that rise into view, the copy buttons, the
// repository links (shown only when the build names a public repository) and the sections that hold
// as scenes.

const REPO = (import.meta.env.VITE_REPO_URL as string | undefined)?.replace(/\/$/, "");

export function initPage() {
  // On a phone the header's pages slide sideways: start with the one you are on in view.
  const tabs = document.querySelector<HTMLElement>(".navlinks");
  const here = tabs?.querySelector<HTMLElement>("[aria-current]");
  if (tabs && here && tabs.scrollWidth > tabs.clientWidth) tabs.scrollLeft = here.offsetLeft - (tabs.clientWidth - here.offsetWidth) / 2;

  // Text arrives: each block rises once, the first time it comes into view.
  const io = new IntersectionObserver((es) => {
    for (const e of es) if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); }
  }, { rootMargin: "0px 0px -8% 0px" });
  document.querySelectorAll(".reveal").forEach((n) => io.observe(n));

  document.querySelectorAll<HTMLButtonElement>("button[data-copy]").forEach((b) => {
    const label = b.textContent;
    let reset = 0;
    b.addEventListener("click", async () => {
      clearTimeout(reset);
      try { await navigator.clipboard.writeText(b.dataset.copy!); b.textContent = "Copied"; }
      catch { b.textContent = "Select and copy"; }
      reset = window.setTimeout(() => (b.textContent = label), 1800);
    });
  });

  // Only /get-started has them.
  const links = document.getElementById("repo-links");
  if (REPO && links) {
    links.querySelectorAll<HTMLAnchorElement>("a[data-repo]").forEach((a) => (a.href = REPO + a.dataset.repo));
    links.hidden = false;
  }

  initScenes();
}
