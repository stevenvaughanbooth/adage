// The words "How it works" leans on, each with a definition for someone meeting it for the first
// time. Captions and the site's math mark the first mention of a term in each block, and a hover,
// focus or tap shows its definition. One list serves the site and `adage map`.

export interface Term {
  id: string;
  /** The forms that count as a mention, matched whole-word and ignoring case. */
  forms: string[];
  def: string;
}

export const TERMS: Term[] = [
  { id: "trace", forms: ["trace", "traces", "transcript", "transcripts"], def: "The record of one agent session: what was asked, then every file read, edit and command, in order. Coding agents already write these as they work." },
  { id: "reflex", forms: ["reflex", "reflexes"], def: "A small learned rule: in this situation, suggest this next step. Each one carries the evidence it was learned from." },
  { id: "instinct", forms: ["instinct", "instincts"], def: "A rule that ships with Adage and works from day one, set up from files your repository already has, like its Makefile or package.json." },
  { id: "hook", forms: ["hook", "hooks", "hook event", "hook events"], def: "A moment when the coding agent lets an outside tool speak: a session starts, a prompt arrives, a command runs, the agent stops. Adage listens there." },
  { id: "directive", forms: ["directive", "directives"], def: "The one-line message Adage hands the agent, such as \"after editing these files, run make gen\"." },
  { id: "fire-bar", forms: ["fire bar"], def: "How sure Adage must be before it tells the agent to take a step rather than suggesting it: a lower bound of 0.80." },
  { id: "fire", forms: ["fire", "fires"], def: "The strongest level: Adage tells the agent to take the step." },
  { id: "prime", forms: ["prime", "primes"], def: "A gentler level: Adage suggests the step and leaves the choice to the agent." },
  { id: "inhibit", forms: ["inhibit", "inhibits"], def: "A warning: this step is a dead end here, and here is what works instead." },
  { id: "dead-end", forms: ["dead end", "dead ends"], def: "A command that keeps failing in this repository, followed by a different one that works." },
  { id: "finish-line", forms: ["finish line", "finish lines"], def: "A step that has to follow a certain edit, like regenerating code after changing a schema." },
  { id: "control", forms: ["control arm", "control session", "control sessions", "control traces", "control trace", "control runs", "control group"], def: "About 1 in 10 sessions where Adage stays silent and only records what it would have said, so the others can be compared with them, like the placebo group in a trial." },
  { id: "uplift", forms: ["uplift"], def: "How much more often agents take a step when told than on their own. A step they would take anyway is not worth saying." },
  { id: "lower-bound", forms: ["wilson lower bound", "lower bound", "lower bounds"], def: "A cautious estimate of a success rate: the lowest the true rate is likely to be, given how few examples there are. 6 out of 8 is 0.75, but its lower bound is only 0.41." },
  { id: "beta", forms: ["beta posterior", "beta posteriors"], def: "A way to estimate a rate that starts from a sensible guess and moves toward what the data says as examples pile up." },
  { id: "embedding", forms: ["embedding", "embeddings", "embedding space"], def: "A list of numbers that places a piece of text on a map of meaning, so things that mean similar things sit close together." },
  { id: "cosine", forms: ["cosine"], def: "How alike two embeddings are: 1 means the same meaning, 0 means unrelated." },
  { id: "louvain", forms: ["louvain"], def: "A standard way to find groups in a network: things that link to each other more than to the rest become one group." },
  { id: "knn", forms: ["knn", "nearest neighbours"], def: "Each item links to the handful of items most similar to it." },
  { id: "mdl", forms: ["compression", "minimum description length", "mdl"], def: "Keep the rules that let you describe the traces most briefly. A rule that saves no space explains nothing new." },
  { id: "calibration", forms: ["calibrated", "calibration", "isotonic regression"], def: "Checking predicted confidence against what actually happened on data held back, then correcting it, so that 80% sure means right 80% of the time." },
  { id: "subgraph", forms: ["subgraph", "subgraphs"], def: "A small piece of the web of events that keeps repeating, like \"edit this file, then run that command\"." },
  { id: "card", forms: ["procedure card", "procedure cards"], def: "A short note for a task the repository keeps repeating: the files to touch, the commands that worked and the check to finish with." },
  { id: "track", forms: ["track", "tracks"], def: "A folder of reflexes with their evidence: one per repository, one for the instincts, one per imported pack." },
  { id: "pack", forms: ["pack", "packs"], def: "A signed bundle of reflexes that a teammate shares." },
  { id: "index", forms: ["index"], def: "One sealed file that holds every active reflex. It is the only thing the hook reads." },
  { id: "sandbox", forms: ["sandbox"], def: "A walled-off space where a command runs without touching your files or the network." },
  { id: "p-value", forms: ["p-value", "p-values"], def: "How often chance alone would give a result at least this big. The smaller it is, the less likely the result is luck." },
  { id: "holm", forms: ["holm's step-down", "holm"], def: "A correction for testing several things at once, so one lucky result does not count as a win." },
  { id: "bootstrap", forms: ["bootstrap"], def: "Resampling the data many times to see how much a result could wobble. It gives a range the true value likely sits in." },
  { id: "sign-flip", forms: ["sign-flip test", "sign-flip"], def: "A test of whether an effect would hold on new tasks: flip each task's result between better and worse, and see how often chance matches what was seen." },
  { id: "propensity", forms: ["propensity"], def: "The chance a session had of landing in its group. Results are weighed by it so the comparison stays fair." },
  { id: "skill", forms: ["skill file", "skill.md", "skill"], def: "A written guide that tells an agent how work is done in a repository." },
  { id: "nascent", forms: ["nascent"], def: "Learned, but held back until a person approves it." },
  { id: "forming", forms: ["forming", "provisional"], def: "Not yet confirmed by enough of your own traces: it can suggest, never insist." },
  { id: "obligation", forms: ["obligation"], def: "Something the agent should still do before it stops, like running the check after an edit." },
  { id: "interlock", forms: ["interlock", "interlocks"], def: "A safety rule that drops any reflex that would recommend a command a dead end says fails." },
  { id: "effect", forms: ["effect class", "effects"], def: "What a step does: observe, verify, regenerate, environment, author or publish. It caps how strongly Adage may ever suggest the step." },
  { id: "support", forms: ["support"], def: "How many separate traces back a pattern." },
  { id: "contrast", forms: ["contrast"], def: "How much more often the step follows its trigger than it happens without it." },
  { id: "template-key", forms: ["template key"], def: "A step with the specifics swapped for placeholders, so \"look up W-1041\" and \"look up W-1042\" count as the same step." },
  { id: "sidechain", forms: ["sidechain"], def: "Records a sub-agent wrote inside the main transcript." },
];

/** A run of text: plain words, or the mention of a term. */
export type Segment = string | { text: string; term: Term };

const byForm = new Map<string, Term>();
for (const t of TERMS) for (const f of t.forms) byForm.set(f.toLowerCase(), t);
const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
// Longest forms first, so "dead end" wins over "end" and "fire bar" over "fire".
const PATTERN = new RegExp(
  `(?<![\\w-])(${[...byForm.keys()].sort((a, b) => b.length - a.length).map(escape).join("|")})(?![\\w-])`,
  "gi",
);

/**
 * Splits text into words and term mentions. Only the first mention of each term is marked; pass
 * one `seen` set across the parts of a block so later parts skip terms already marked.
 */
export function segments(text: string, seen: Set<string> = new Set()): Segment[] {
  const out: Segment[] = [];
  let at = 0;
  for (const m of text.matchAll(PATTERN)) {
    const term = byForm.get(m[0].toLowerCase())!;
    if (seen.has(term.id)) continue;
    seen.add(term.id);
    if (m.index > at) out.push(text.slice(at, m.index));
    out.push({ text: m[0], term });
    at = m.index + m[0].length;
  }
  if (at < text.length) out.push(text.slice(at));
  return out;
}
