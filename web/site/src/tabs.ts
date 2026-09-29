// The hero's tabs: each part of the agent, in the words the panel shows and the scene state it frames.
export type Tab = { id: string; name: string; dot: string; title: string; body: string; facts: string[] };

export const TABS: Tab[] = [
  { id: "overview", name: "Overview", dot: "var(--ink-1)",
    title: "Three ways an agent knows what to do",
    body: "Context is what the model recalls by inference: CLAUDE.md, skills and tool definitions, weighed on every turn. Instincts ship with Adage. Reflexes are learned from your agents' traces. Both live outside the context window and speak up only when they match.",
    facts: ["context: recalled by inference", "instincts: built in, day one", "reflexes: learned from traces"] },
  { id: "context", name: "Context", dot: "var(--memory)",
    title: "Context is recalled by inference",
    body: "CLAUDE.md, the system prompt, skill descriptions and MCP tool definitions sit in the context window. On every turn the model has to notice them, weigh them and choose. They cost tokens, and in a long session they get missed.",
    facts: ["lives in: the context window", "recalled by: inference", "costs: tokens, every turn"] },
  { id: "instincts", name: "Instincts", dot: "var(--prime)",
    title: "Instincts work on day one",
    body: "Adage ships with instincts bound from your repository's own files: the Makefile, package.json, lockfiles and generated-file markers. They need no history. Instincts only prime, and a reflex learned for the same step replaces its instinct.",
    facts: ["7 in the default set", "bound from: Makefile · package.json · lockfiles", "strength: prime at most"] },
  { id: "reflexes", name: "Reflexes", dot: "var(--fire)",
    title: "Reflexes are learned from traces",
    body: "Adage reads what your agents did and keeps the steps that got work done. When the same moment comes back, a hook matches it and hands the agent a one-line directive, so the model doesn't have to recall it. Dead ends become inhibit reflexes.",
    facts: ["learned from: your agents' traces", "fire · prime · inhibit", "10% of sessions are a control"] },
  { id: "agent", name: "At the hooks", dot: "var(--memory-soft)",
    title: "Where Adage fits",
    body: "Your agent keeps its model, context and tools. Adage runs at the harness's hooks, outside the model: when an event matches a reflex, the directive arrives as text at that moment. Reflexes you approve for delegation can run the step themselves. If Adage fails, the agent carries on as if it were not there.",
    facts: ["Claude Code · Codex and OpenCode in preview", "runs outside the model", "fails open"] },
];

export const SCENE_OF: Record<string, string> = { overview: "overview", context: "memory", instincts: "instincts", reflexes: "reflexes", agent: "agent" };
