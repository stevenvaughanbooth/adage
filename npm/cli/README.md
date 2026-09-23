# Adage

**Agentic DAG Engine**: put your agent on rails.

Adage learns the routes coding agents take to get work done, packages the good ones as shareable
**tracks**, and at each decision point tells the agent, deterministically, what to do next. It never
executes anything itself.

This package installs the `adage` command. The binary comes from a per-platform package that npm
selects by `os` and `cpu`; nothing is downloaded at install time, so `--ignore-scripts`, offline
mirrors and proxies all work.

```bash
adage learn          # learn from your Claude Code history
adage status
adage install claude-code    # or opencode, codex: print the hook registration
```

Documentation, source and issues: <https://github.com/stevenvaughanbooth/adage>.
