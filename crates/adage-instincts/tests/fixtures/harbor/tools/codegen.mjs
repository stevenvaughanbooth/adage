import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const verify = args.includes("--verify");
const at = args.indexOf("--root");
const root = at >= 0 ? resolve(args[at + 1]) : join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = "src/generated/registry.ts";

function fail(message) {
  console.error(`codegen: ${message}`);
  process.exit(1);
}

function readCsv(rel, columns) {
  const text = readFileSync(join(root, rel), "utf8");
  const lines = text.split(/\r?\n/).map((l) => l.trim()).filter((l) => l && !l.startsWith("#"));
  const header = lines[0].split(",").map((c) => c.trim());
  if (header.join(",") !== columns.join(",")) {
    fail(`${rel}: expected columns ${columns.join(",")}, found ${header.join(",")}`);
  }
  return lines.slice(1).map((line, i) => {
    const cells = line.split(",").map((c) => c.trim());
    if (cells.length !== header.length) {
      fail(`${rel}:${i + 2}: expected ${header.length} fields, found ${cells.length}`);
    }
    return Object.fromEntries(header.map((h, j) => [h, cells[j]]));
  });
}

function num(rel, row, key) {
  const n = Number(row[key]);
  if (row[key] === "" || !Number.isFinite(n)) fail(`${rel}: ${key} is not a number: ${row[key]}`);
  return n;
}

function str(value) {
  return JSON.stringify(value);
}

const ports = readCsv("registry/ports.csv", ["code", "name", "country", "timezone", "max_draft_m", "pilotage_hours"])
  .map((r) => ({
    code: r.code.toUpperCase(),
    name: r.name,
    country: r.country.toUpperCase(),
    timezone: r.timezone,
    maxDraftM: num("registry/ports.csv", r, "max_draft_m"),
    pilotageHours: num("registry/ports.csv", r, "pilotage_hours"),
  }))
  .sort((a, b) => a.code.localeCompare(b.code));

const vessels = readCsv("registry/vessels.csv", ["class", "length_m", "beam_m", "draft_m", "teu"])
  .map((r) => ({
    name: r.class,
    lengthM: num("registry/vessels.csv", r, "length_m"),
    beamM: num("registry/vessels.csv", r, "beam_m"),
    draftM: num("registry/vessels.csv", r, "draft_m"),
    teu: num("registry/vessels.csv", r, "teu"),
  }))
  .sort((a, b) => a.lengthM - b.lengthM || a.name.localeCompare(b.name));

const seen = new Set();
for (const p of ports) {
  if (seen.has(p.code)) fail(`registry/ports.csv: duplicate code ${p.code}`);
  seen.add(p.code);
}

const lines = [
  'import type { Port, VesselClass } from "../types.ts";',
  "",
  "export const PORTS: readonly Port[] = [",
  ...ports.map(
    (p) =>
      `  { code: ${str(p.code)}, name: ${str(p.name)}, country: ${str(p.country)}, timezone: ${str(p.timezone)}, maxDraftM: ${p.maxDraftM}, pilotageHours: ${p.pilotageHours} },`,
  ),
  "];",
  "",
  "export const VESSEL_CLASSES: readonly VesselClass[] = [",
  ...vessels.map(
    (v) =>
      `  { name: ${str(v.name)}, lengthM: ${v.lengthM}, beamM: ${v.beamM}, draftM: ${v.draftM}, teu: ${v.teu} },`,
  ),
  "];",
  "",
];
const text = lines.join("\n");

if (verify) {
  let current = "";
  try {
    current = readFileSync(join(root, OUT), "utf8");
  } catch {
    current = "";
  }
  if (current !== text) {
    console.error(`check: ${OUT} is out of date with registry/*.csv (run: npm run codegen)`);
    process.exit(1);
  }
} else {
  writeFileSync(join(root, OUT), text);
  console.log(`wrote ${OUT} (${ports.length} ports, ${vessels.length} vessel classes)`);
}
