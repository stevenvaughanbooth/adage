// /how's opening scene: "How Adage plugs into Claude Code". The page scrolls as it always does; the
// diagram holds beside the steps (CSS position: sticky) and lights the hook of whichever step is in
// the middle of the screen. Nothing here moves the scroll. Without this script every part of the
// diagram shows at once.

export function initPlug() {
  const fig = document.getElementById("plug-fig");
  const steps = [...document.querySelectorAll<HTMLElement>(".plug-step")];
  if (!fig || !steps.length) return;
  const parts = [...fig.querySelectorAll<SVGElement>("[data-hook]")];
  const caps = [...fig.querySelectorAll<SVGElement>("[data-cap]")];

  const show = (step: string) => {
    if (fig.dataset.step === step && fig.classList.contains("live")) return;
    fig.dataset.step = step;
    fig.classList.add("live");
    for (const p of parts) p.classList.toggle("on", p.dataset.hook!.split(" ").includes(step));
    for (const c of caps) c.classList.toggle("on", c.dataset.cap === step);
    for (const s of steps) s.classList.toggle("on", s.dataset.step === step);
  };

  // The step crossing the middle of the screen is the one shown.
  const io = new IntersectionObserver((es) => {
    for (const e of es) if (e.isIntersecting) show((e.target as HTMLElement).dataset.step!);
  }, { rootMargin: "-45% 0px -45% 0px" });
  steps.forEach((s) => io.observe(s));
  show(steps[0]!.dataset.step!);
}
