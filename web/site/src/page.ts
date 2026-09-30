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

  const form = document.getElementById("signup") as HTMLFormElement | null;
  if (!form) return;
  const status = document.getElementById("signup-status")!;
  const email = form.elements.namedItem("email") as HTMLInputElement;
  const say = (msg: string, kind: "" | "ok" | "err") => { status.textContent = msg; status.className = "status " + kind; };
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    if (!email.value.trim() || !email.validity.valid) { say("Enter an email address, like you@company.com.", "err"); email.focus(); return; }
    const button = form.querySelector("button")!;
    button.disabled = true; say("Adding you…", "");
    try {
      const r = await fetch(form.action, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ email: email.value.trim(), hp_x7: (form.elements.namedItem("hp_x7") as HTMLInputElement).value }),
      });
      if (r.ok) { form.reset(); say("You're on the list. We'll email you once, when the first release is ready.", "ok"); }
      else if (r.status === 400 || r.status === 413) say("That address doesn't look right. Check it and try again.", "err");
      else if (r.status === 503) say("Sign-ups aren't open yet. You can explore the source preview below.", "err");
      else say("Something went wrong on our side. Please try again in a minute.", "err");
    } catch {
      say("Couldn't reach the server. Check your connection and try again.", "err");
    } finally {
      button.disabled = false;
    }
  });
}
