// npm distribution settings, read by npm/scripts/stage.mjs and the release workflow.

// TODO(D3): the npm package name is undecided (Implementation Plan §6.2 and §9, decision D3).
// `@adage/cli` is a placeholder: the `@adage` scope has not been claimed, `adage` is taken, and
// `adage-cli` is the fallback. This is the only place the name is set. Platform packages are named
// `<PACKAGE_NAME>-<os>-<cpu>` (`@adage/cli-darwin-arm64`); the command is `adage` whatever the name.
export const PACKAGE_NAME = "@adage/cli"

// Release targets (dist-workspace.toml) and the npm platform package each one becomes. Linux builds
// are static musl binaries, so one package per CPU serves glibc and musl systems alike.
export const PLATFORMS = [
  { target: "aarch64-apple-darwin", os: "darwin", cpu: "arm64", exe: "adage" },
  { target: "x86_64-apple-darwin", os: "darwin", cpu: "x64", exe: "adage" },
  { target: "aarch64-unknown-linux-musl", os: "linux", cpu: "arm64", exe: "adage" },
  { target: "x86_64-unknown-linux-musl", os: "linux", cpu: "x64", exe: "adage" },
  { target: "x86_64-pc-windows-msvc", os: "win32", cpu: "x64", exe: "adage.exe" },
]
