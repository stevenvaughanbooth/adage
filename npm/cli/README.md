# Adage

**Agent traces become reflexes.** (Adage: *Agentic DAG Engine*)

Adage reads the traces coding agents leave behind, conditions the paths that get work done into
**reflexes** (kept in **tracks**, shared as signed **packs**), and at each decision point tells the
agent, deterministically, what to do next. By default it only advises. Running a command in the
agent's place is opt-in, one approved reflex at a time, and a person's decision: `adage delegate on`
at a terminal, or an answer in the Claude Code session.

This package installs the `adage` command, with its two companions beside it in the platform
package npm selects by `os` and `cpu`: `adage-exec`, which runs only the reflexes you let act
(macOS and Linux), and `adage-train`, which compiles a skill into reflexes. Nothing is downloaded at
install time, so `--ignore-scripts`, offline mirrors and proxies all work.

```bash
npm install -g @adage-sh/cli
adage init      # once, in your repository: sets up Claude Code (and Codex or OpenCode if present), starts learning
adage update    # later: updates Adage, its hooks and the Claude Code plugin together; --rollback goes back
```

`adage init` installs the Adage plugin into Claude Code for you, writes the Codex and OpenCode hook
files it needs (each shown first, the previous one kept as `<file>.adage-bak`), and ends on one
screen with the line your next session opens with. When a newer Adage is out, the status card
(bare `adage`) says so, and `adage update` runs `npm install --global @adage-sh/cli@<version>` for
you, keeping the previous binaries for `adage update --rollback`.

Documentation, source and issues: <https://github.com/stevenvaughanbooth/adage>.
