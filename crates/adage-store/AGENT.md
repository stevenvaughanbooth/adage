# adage-store

Daemonless file storage under `$ADAGE_HOME`: two-slot session state files, CRC-framed append-only
spools, single-flight file locks, the `config.toml` loader, and the `Paths` layout every surface
uses to find them.

## Where it sits

- Layer 0 in `xtask/src/layers.rs` (kernel primitives). It links no workspace crate; external
  dependencies are serde, serde_json and toml only.
- Linked (`[dependencies]`) by `adage-app`, `adage-cli`, `adage-exec`, `adage-hook`,
  `adage-learn`, `adage-train`; `adage-eval` uses it as a dev-dependency.
- Two `FORBIDDEN` edges point at it: `adage-ingest -> adage-store` ("ingestion reads traces,
  never Adage's own state") and `adage-runtime -> adage-store` ("the runtime decides; the hook
  does the I/O"). Do not add it to either of those crates.

## What's inside

- `paths`: `Paths { root }` with `from_env()` (`ADAGE_HOME`, else the platform data directory),
  `at(root)`, and one method per file or directory: `sessions`, `state(session)`,
  `spool(session)`, `index`, `tracks`, `models`, `repo_keys`, `learn_log`, `config`,
  `learn_queue`, `ledger`, `cache`, `tracks_lock`. `home_dir()`; `sanitise(session_id)`.
- `state`: `SLOT` (64 KiB), `update`, `update_or_rebuild`, `try_read`, `read`. Each slot is
  `magic | seq | len | crc | payload`; the newest slot with a valid CRC wins.
- `spool`: `MAX_RECORD` (1 MiB), `append`, `append_durable`, `read_all`, `read_all_strict`,
  `read_from(path, offset)` for tailing. Frames are `magic | len | crc | payload`.
- `lock`: `Lock` (released on drop), `try_exclusive`, `exclusive_within(path, patience)`,
  `exclusive`, `is_held`.
- `config`: `FILE`, `Config`, `Thresholds`, `Source`, `Key`/`KEYS`/`key(name)`, `Loaded
  { config, values, problems }`, `load(&Paths)`, `set(&Paths, name, value)`, `write_atomic`.
- `crc`: private `crc32`.

## How it connects

- No traits: this crate is the file formats and the directory layout. Everything that reads or
  writes `$ADAGE_HOME` goes through `Paths` rather than joining strings.
- On the hook path, `adage-hook` updates a session's `.state` with `state::update` and appends a
  decision record to its `.spool` with `spool::append`; `adage ui` and `adage watch` tail spools
  with `spool::read_from`. Spool records carry no prompt text.
- `adage-learn` takes `lock::exclusive` on `learn-queue/<repo>.lock` (one learner per repository)
  and `tracks.lock` (one writer of tracks and index); `adage_app::review` takes the same lock.
- `config::load` is called by `adage-cli`, `adage-app` and `adage-hook`; `Loaded.problems`
  carries anything wrong with the file so a command can print it.
- `append_durable` and `read_all_strict` exist for a spool used as an execution journal, where
  no torn or corrupt record may be skipped silently; the ordinary telemetry writer keeps the
  cheaper semantics. See [docs/architecture.md](../../docs/architecture.md), "Store".

## Changing it

- All tests are unit tests inside the modules: `cargo test -p adage-store`. CI runs that on every
  platform in its own step (`ci.yml`: "the Windows-ported store"), before the affected set. Then
  run the readers: `cargo test -p adage-hook -p adage-learn -p adage-app -p adage-cli`. It reads
  no fixtures; tests use `tempfile` and `ADAGE_HOME`.
- The state CRC covers `seq` and `len` as well as the payload; the comment on `frame_crc`
  records the torn-write bug a payload-only CRC caused. A payload larger than `SLOT - HEADER` is
  an error, not a truncation. State is a cache: it can be rebuilt (`update_or_rebuild`).
- A spool record is one `O_APPEND` write (`FILE_APPEND_DATA` on Windows), so concurrent writers
  never interleave. Readers stop at an incomplete frame and resynchronise on the next magic;
  `read_from` returns the offset of the first unread complete frame.
- Locks use `std::fs::File::lock` (`flock` on Unix, `LockFileEx` on Windows) and are released by
  the OS when a holder dies. The lock file's presence means nothing; `is_held` never creates it.
- Positional I/O is looped on Windows (`seek_read`/`seek_write` may transfer less than asked);
  keep platform differences behind `cfg(unix)` / `cfg(windows)` as the existing `pos` module does.
- Session ids come from harness payloads: `sanitise` keeps them inside `sessions/` and adds a
  hash suffix when it had to alter one, so two ids never share files.
- Anything the hook calls must fail open ([CONTRIBUTING.md](../../CONTRIBUTING.md)): return the
  `io::Result`, never panic.
