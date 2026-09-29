// npm distribution settings, read by npm/scripts/stage.mjs and the release workflow.

// The npm package name (decision D3): `@adage/cli`, once the `@adage` scope is claimed
// (`npm org create adage`). `adage` itself is taken; if the scope is not available, use the
// fallback `adage-cli`. This is the only place the name is set. Platform packages are named
// `<PACKAGE_NAME>-<os>-<cpu>` (`@adage/cli-darwin-arm64`); the command is `adage` whatever the name.
export const PACKAGE_NAME = "@adage/cli"

// Release targets (dist-workspace.toml) and the npm platform package each one becomes. Linux builds
// are static musl binaries, so one package per CPU serves glibc and musl systems alike.
// Binaries shipped beside `adage` in each platform package, each from its own release archive
// (`<app>-<target>`). `adage delegate on` finds adage-exec beside the running adage, and `adage
// train` finds adage-train there. Execution (adage-exec) needs an OS sandbox, which Windows lacks
// (E0 only there), so it is not required on Windows.
export const COMPANIONS = [
  { name: "adage-exec", app: "adage-exec", required: (p) => p.os !== "win32" },
  { name: "adage-train", app: "adage-train", required: () => true },
]

export const PLATFORMS = [
  { target: "aarch64-apple-darwin", os: "darwin", cpu: "arm64", exe: "adage" },
  { target: "x86_64-apple-darwin", os: "darwin", cpu: "x64", exe: "adage" },
  { target: "aarch64-unknown-linux-musl", os: "linux", cpu: "arm64", exe: "adage" },
  { target: "x86_64-unknown-linux-musl", os: "linux", cpu: "x64", exe: "adage" },
  { target: "x86_64-pc-windows-msvc", os: "win32", cpu: "x64", exe: "adage.exe" },
]
