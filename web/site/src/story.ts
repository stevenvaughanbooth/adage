// A concrete, deliberately illustrative bridge from the cinematic hero to the mechanism.
const STEPS = [
  { mode: "Trace · exploring the repository", caption: "Session 1 / discovery", code: "edit schema/event.json\n$ make check\n  generated files are out of date\n$ make gen\n$ make check\n  checks pass", detail: "The trace records the recovery. Across sessions, a repeatable edit → regenerate pattern builds evidence." },
  { mode: "Reflex · a path learned from repeated work", caption: "Between sessions / conditioning", code: "trigger   edit schema/*.json\nstep      make gen\nsource    repeated agent traces\nreview    evidence attached", detail: "Adage records a rule with a specific trigger. Review its supporting sessions and choose whether to keep the guidance." },
  { mode: "Hook · local match, useful step", caption: "Next session / the shortcut", code: "edit schema/notification.json\n↳ Adage: Changes to schema/*.json\n  must be followed by make gen.\n$ make gen\n$ make check\n  checks pass", detail: "The hook matches the edit and suggests the recorded step. The agent decides how to use it; a suggestion alone does not prove a saved turn." },
];

export function initStory() {
  const root = document.querySelector<HTMLElement>(".reflex-story")!;
  const buttons = [...root.querySelectorAll<HTMLButtonElement>("[data-story]")];
  for (const [i, button] of buttons.entries()) {
    button.addEventListener("click", () => {
      const step = STEPS[i]!;
      root.dataset.stage = String(i);
      buttons.forEach((b, n) => b.setAttribute("aria-pressed", String(n === i)));
      for (const [id, value] of Object.entries({ "story-mode": step.mode, "story-caption": step.caption, "story-code": step.code, "story-detail": step.detail })) {
        document.getElementById(id)!.textContent = value;
      }
    });
  }
}
