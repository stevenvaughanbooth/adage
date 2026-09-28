# Adage

**Agent traces become reflexes.** (Adage: *Agentic DAG Engine*)

Adage reads the traces coding agents leave behind, conditions the paths that get work done into
**reflexes** (kept in **tracks**, shared as signed **packs**), and at each decision point tells the
agent, deterministically, what to do next. It never executes anything itself.

This package installs the `adage` command. The binary comes from a per-platform package that npm
selects by `os` and `cpu`; nothing is downloaded at install time, so `--ignore-scripts`, offline
mirrors and proxies all work.

```bash
adage sense              # read-only: which reflexes your Claude Code traces would form
adage condition          # condition reflexes on those traces
adage status
adage install claude-code    # or opencode, codex: print the hook registration
```

Documentation, source and issues: <https://github.com/stevenvaughanbooth/adage>.
