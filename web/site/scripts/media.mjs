// `pnpm media <night.mp4> <day.mp4> [--upload]`: prepare the teaser films for the site.
//
// Encodes each cut to 1080p H.264 at about 1.25 Mbit/s (two-pass, fast start, ~14 MB for 88 s) into
// public/media/teaser-{dark,day}.mp4, which `pnpm dev` and `pnpm preview` serve and git ignores, and
// refreshes the committed posters (public/media/poster-*.jpg, the first frame).
//
// --upload also puts both films in a Vercel Blob store (needs BLOB_READ_WRITE_TOKEN, from the store's
// settings) and prints the VITE_MEDIA_BASE to set on the Vercel project. Needs ffmpeg on PATH.

import { execFileSync } from "node:child_process";
import { readFileSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";

const here = dirname(new URL(import.meta.url).pathname);
const media = join(here, "../public/media");
const args = process.argv.slice(2);
const upload = args.includes("--upload");
const [night, day] = args.filter((a) => !a.startsWith("--"));
if (!night || !day) {
  console.error("usage: pnpm media <night.mp4> <day.mp4> [--upload]");
  process.exit(2);
}

const ff = (...a) => execFileSync("ffmpeg", ["-v", "error", "-y", ...a], { stdio: "inherit" });
function encode(src, name) {
  const out = join(media, `teaser-${name}.mp4`), log = join(tmpdir(), `adage-teaser-${name}`);
  const v = ["-c:v", "libx264", "-preset", "slow", "-b:v", "1150k", "-passlogfile", log];
  console.log(`encoding ${name} (two passes)…`);
  ff("-i", src, ...v, "-pass", "1", "-an", "-f", "mp4", "/dev/null");
  ff("-i", src, ...v, "-maxrate", "2000k", "-bufsize", "3000k", "-pass", "2", "-pix_fmt", "yuv420p", "-profile:v", "high", "-c:a", "aac", "-b:a", "96k", "-movflags", "+faststart", out);
  for (const f of [`${log}-0.log`, `${log}-0.log.mbtree`]) rmSync(f, { force: true });
  ff("-ss", "0.5", "-i", out, "-frames:v", "1", "-q:v", "4", join(media, `poster-${name}.jpg`));
  return out;
}

const files = { dark: encode(night, "dark"), day: encode(day, "day") };

if (upload) {
  if (!process.env.BLOB_READ_WRITE_TOKEN) {
    console.error("--upload needs BLOB_READ_WRITE_TOKEN (Vercel → Storage → your Blob store → .env.local)");
    process.exit(2);
  }
  const { put } = await import("@vercel/blob");
  let base = "";
  for (const [name, path] of Object.entries(files)) {
    const r = await put(`media/teaser-${name}.mp4`, readFileSync(path), { access: "public", contentType: "video/mp4", addRandomSuffix: false, allowOverwrite: true, cacheControlMaxAge: 604800 });
    console.log(`uploaded ${r.url}`);
    base = r.url.slice(0, r.url.lastIndexOf("/") + 1);
  }
  console.log(`\nSet this on the Vercel project (Settings → Environment Variables), then redeploy:\n  VITE_MEDIA_BASE=${base}`);
}
