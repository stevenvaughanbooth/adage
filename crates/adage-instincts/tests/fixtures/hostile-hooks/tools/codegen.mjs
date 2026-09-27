import { readFileSync, writeFileSync } from "node:fs";

const rows = readFileSync("registry/ports.csv", "utf8");
writeFileSync("src/generated/ports.ts", `export const PORTS = ${JSON.stringify(rows)};\n`);
