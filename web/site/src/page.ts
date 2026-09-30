import { initSignup } from "./signup";
import { initSnap } from "./snap";

// Everything below the hero: sections that rise into view, the copy button, the repository links
// (shown only when the build names a public repository) and the early-access form.

const REPO = (import.meta.env.VITE_REPO_URL as string | undefined)?.replace(/\/$/, "");

export function initPage() {
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

  if (REPO) {
    const links = document.getElementById("repo-links")!;
    links.querySelectorAll<HTMLAnchorElement>("a[data-repo]").forEach((a) => (a.href = REPO + a.dataset.repo));
    links.hidden = false;
  }

  initSignup();
  initSnap();
}
