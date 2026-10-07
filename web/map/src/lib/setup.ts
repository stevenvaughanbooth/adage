import type { Graph } from "../api/types";

// What the map tells a person to do next, worded for the surface they use (onboarding plan O10):
// a Claude Code plugin user acts through `/adage` in Claude Code and never needs a terminal for it;
// anyone else has the `adage` commands. The binary says which in the graph's `adage.channel`; an
// older binary sends nothing, and the terminal words stay.

/** The binary serving this page is the Claude Code plugin's. */
export function isPlugin(graph: Graph | null): boolean {
  return graph?.adage?.channel === "plugin";
}

export interface SetupStep {
  title: string;
  /** The one command or `/adage` verb that does it. */
  action: string;
  detail: string;
}

export interface SetupCopy {
  /** Where the person acts, in one line. */
  lead: string;
  /** What to do first, under it. */
  intro: string;
  steps: SetupStep[];
  /** How a reflex selected on the map is decided. */
  review: (id: string) => string;
}

const PLUGIN: SetupCopy = {
  lead: "In Claude Code: /adage review, /adage health; the pane's Explore in browser opened this page.",
  intro:
    "Adage runs through its Claude Code plugin: the band above the prompt, the pane and /adage. At the end of a turn it asks before a new reflex speaks to the agent; you answer there. This page explains the evidence and shows live traces; it changes nothing.",
  steps: [
    {
      title: "Connected",
      action: "/adage",
      detail:
        "The plugin wires every hook; each session's first line says what Adage is doing. /adage shows the card and opens the pane.",
    },
    {
      title: "Review what Adage learned",
      action: "/adage review",
      detail:
        "Each reflex waiting for you, with its evidence and the exact words the agent would get, before you enable it.",
    },
    {
      title: "Check health",
      action: "/adage health",
      detail: "Hooks, the index, learning and the embedding model, each with its fix.",
    },
    {
      title: "Stay current",
      action: "/adage update",
      detail: "When a new Adage is out, the band says so once; the update takes effect in your next session.",
    },
  ],
  review: (id) => `To decide on ${id}, open /adage review in Claude Code: it shows this reflex's evidence and the exact change.`,
};

const TERMINAL: SetupCopy = {
  lead: "In a terminal: adage ui, adage review, adage update.",
  intro:
    "adage init connects every agent found once; adage ui then has every action in its palette, each with its confirmation. This page explains the evidence and shows live traces; it changes nothing.",
  steps: [
    {
      title: "Connect your agents",
      action: "adage init",
      detail:
        "Connects every agent found: Claude Code through the Adage plugin, Codex and OpenCode through their hook files. Each change is shown before it is written. For Codex, accept the registered hooks in /hooks.",
    },
    {
      title: "Check earlier sessions",
      action: "adage ui",
      detail:
        "History sources in the palette shows the detected Claude Code and Codex histories, or sets a history directory. For OpenCode, Import OpenCode history copies local exports.",
    },
    {
      title: "Learn, then review",
      action: "adage review",
      detail:
        "Condition now in adage ui previews which sessions are read. Review the reflexes it proposes, with their exact before-and-after responses, before accepting a change.",
    },
    {
      title: "Stay current",
      action: "adage update",
      detail: "Updates Adage, its hooks and the plugin together, and can go back; adage doctor if anything looks wrong.",
    },
  ],
  review: (id) => `To decide on ${id}, select it in adage ui's Reflexes view and choose Review, or run adage review: the confirmation shows the exact change for this installed version.`,
};

export function setupCopy(graph: Graph | null): SetupCopy {
  return isPlugin(graph) ? PLUGIN : TERMINAL;
}

/** The words of a button that names a command the person runs (the map runs nothing). */
export function actionWords(graph: Graph | null): { manage: string; guide: string; alternative: string } {
  return isPlugin(graph)
    ? { manage: "Do this from Claude Code with /adage", guide: "What to type in Claude Code →", alternative: "Terminal alternative" }
    : { manage: "Manage this in the terminal", guide: "Open terminal action guide →", alternative: "CLI alternative" };
}

/** The palette's footer hint. */
export function paletteHint(graph: Graph | null): string {
  return isPlugin(graph)
    ? "Use /adage in Claude Code for changes; the map explains each step"
    : "Use the terminal palette for changes; the map explains each step";
}

/** The version beside the wordmark, and a newer release when one is known. */
export function versionText(graph: Graph | null): { version: string; update: string | null } | null {
  const a = graph?.adage;
  if (!a?.version) return null;
  const update = a.latest && a.latest !== a.version ? `${a.latest} is out · ${isPlugin(graph) ? "/adage update" : "adage update"}` : null;
  return { version: `adage ${a.version}`, update };
}

/** The map palette's hints beside its setup entries. */
export function paletteWords(graph: Graph | null): { guide: string; review: string; condition: string } {
  return isPlugin(graph)
    ? { guide: "what to do in Claude Code", review: "/adage review in Claude Code", condition: "learns as you work" }
    : { guide: "terminal palette guide", review: "review in terminal palette", condition: "preview sources in terminal" };
}

/** The first-run view's line under its setup button. */
export function firstRunHint(graph: Graph | null): string {
  return isPlugin(graph)
    ? "Adage learns from your sessions as you work: answer its asks at the end of a turn, and /adage review shows what waits for you."
    : "Connect your agent, check history sources, and preview conditioning from the terminal palette. OpenCode history is imported explicitly.";
}

/** Why an execution-only reflex's review is a person's, and where it is made. */
export function executionReviewWhy(graph: Graph | null): string {
  return isPlugin(graph)
    ? "Review and approve the exact automatic action with /adage review in Claude Code."
    : "Review and approve the exact automatic action at your terminal.";
}
