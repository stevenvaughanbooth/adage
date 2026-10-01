// What a pack is, file by file (the Packs section): a reflex a team shares and an instinct Adage
// ships, as the files on disk. Both are real. The reflex pack was shared from the acme-billing
// study (evals/datasets/live-20260928-acme-billing-sonnet5/packs/work-items); the instinct is
// Adage's own packs/python-env, as .adage/marketplace.toml pins it. "Fill in the blanks" shows
// one route detector U11 bound from a Python project with a .venv (13 in all, one per way an agent
// can miss the environment), as `cargo run -p adage-instincts --example dry_run` prints it.

import reflexManifest from "@repo/evals/datasets/live-20260928-acme-billing-sonnet5/packs/work-items/pack.toml?raw";
import reflexRoute from "@repo/evals/datasets/live-20260928-acme-billing-sonnet5/packs/work-items/routes/read-linked-files.yaml?raw";
import reflexEvidence from "@repo/evals/datasets/live-20260928-acme-billing-sonnet5/packs/work-items/evidence.json?raw";
import reflexClosure from "@repo/evals/datasets/live-20260928-acme-billing-sonnet5/packs/work-items/CLOSURE?raw";
import reflexSig from "@repo/evals/datasets/live-20260928-acme-billing-sonnet5/packs/work-items/pack.sig?raw";
import instinctManifest from "@repo/packs/python-env/pack.toml?raw";
import instinctRun from "@repo/packs/python-env/routes/run-tests-in-project-env.yaml?raw";
import instinctAdvice from "@repo/packs/python-env/routes/name-project-interpreter.yaml?raw";
import instinctClosure from "@repo/packs/python-env/CLOSURE?raw";

type PackFile = { path: string; what: string; code: string };
type PackView = { files: PackFile[]; blanks: boolean };

const PACKS: Record<string, PackView> = {
  reflex: {
    blanks: false,
    files: [
      {
        path: "pack.toml",
        what: "The label on the box: its name and version, where it applies (one repository here), what it needs, and the most it may ever ask to run (E0: advice only).",
        code: reflexManifest,
      },
      {
        path: "routes/read-linked-files.yaml",
        what: "One reflex. After the agent looks a work item up, read the files it links (up to five) together, in one turn. The numbers under evidence say how sure Adage is: seen 3 times out of 3.",
        code: reflexRoute,
      },
      {
        path: "evidence.json",
        what: "Where the evidence came from: three observations of real agent sessions (organic), none made up.",
        code: reflexEvidence,
      },
      {
        path: "CLOSURE",
        what: "Everything the pack depends on, written out. The recipient's Adage works it out again from the files and refuses the pack if anything is hidden or missing.",
        code: reflexClosure,
      },
      {
        path: "pack.sig",
        what: "The sender's SSH signature over the whole pack. Change one byte and it no longer matches. Each recipient chooses whose signatures to trust.",
        code: reflexSig,
      },
    ],
  },
  instinct: {
    blanks: true,
    files: [
      {
        path: "pack.toml",
        what: "The same format. Its origin is instinct, and it applies wherever detector U11 finds a project's Python environment. There is no pack.sig: Adage's own marketplace pins this pack's digest, and the binary checks it.",
        code: instinctManifest,
      },
      {
        path: "routes/run-tests-in-project-env.yaml",
        what: "A template: a reflex with blanks. Detector U11, built into Adage, fills them in from your repository's files: which command failed, how, and what to run instead.",
        code: instinctRun,
      },
      {
        path: "routes/name-project-interpreter.yaml",
        what: "Advice, from one fixed sentence. Its only blank is the interpreter to use, such as .venv/bin/python.",
        code: instinctAdvice,
      },
      {
        path: "CLOSURE",
        what: "It lists detector U11 as a dependency, the way it lists its templates: the detector ships inside Adage, so the pack names it and never carries code.",
        code: instinctClosure,
      },
    ],
  },
};

// The blanks, and what U11 filled them with in a Python project with a .venv.
const FILLED: Record<string, string> = {
  "{runner}": "exec python3 -m pytest",
  "{signature}": "No module named pytest",
  "{command}": ".venv/bin/python -m pytest",
  "{interpreter}": ".venv/bin/python",
};
const BLANK = /\{(?:runner|signature|command|interpreter)\}/g;

export function initPacks() {
  const root = document.getElementById("pack-explorer");
  if (!root) return;
  const picks = [...root.querySelectorAll<HTMLButtonElement>("[data-pack]")];
  const list = document.getElementById("pack-files")!;
  const what = document.getElementById("pack-what")!;
  const code = document.getElementById("pack-code")!;
  const fillRow = document.getElementById("pack-fill-row")!;
  const fill = document.getElementById("pack-fill") as HTMLButtonElement;
  let pack = PACKS.reflex!;
  let file = 0;
  let filled = false;

  function show() {
    const f = pack.files[file]!;
    what.textContent = f.what;
    const text = f.code.trimEnd();
    // Blanks are marked; filled in, each shows what the detector put there.
    const parts: Node[] = [];
    let last = 0;
    for (const m of pack.blanks ? text.matchAll(BLANK) : []) {
      parts.push(document.createTextNode(text.slice(last, m.index)));
      const mark = document.createElement("mark");
      mark.className = filled ? "filled" : "blank";
      mark.textContent = filled ? FILLED[m[0]]! : m[0];
      parts.push(mark);
      last = m.index! + m[0].length;
    }
    parts.push(document.createTextNode(text.slice(last)));
    code.replaceChildren(...parts);
    const has = pack.blanks && text.search(BLANK) >= 0;
    fillRow.hidden = !has;
    fill.setAttribute("aria-pressed", String(filled));
    fill.textContent = filled ? "Show the blanks" : "Fill in the blanks";
    list.querySelectorAll("button").forEach((b, i) => b.setAttribute("aria-pressed", String(i === file)));
  }

  function open(name: string) {
    pack = PACKS[name]!;
    file = 0;
    filled = false;
    picks.forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.pack === name)));
    root!.dataset.pack = name;
    list.replaceChildren(
      ...pack.files.map((f, i) => {
        const b = document.createElement("button");
        b.type = "button";
        b.textContent = f.path;
        b.setAttribute("aria-controls", "pack-code");
        b.addEventListener("click", () => {
          file = i;
          show();
        });
        return b;
      }),
    );
    show();
  }

  picks.forEach((b) => b.addEventListener("click", () => open(b.dataset.pack!)));
  fill.addEventListener("click", () => {
    filled = !filled;
    show();
  });
  open("reflex");
}
