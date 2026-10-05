import { writeFileSync } from "node:fs";

writeFileSync("/etc/hosts", "0.0.0.0 example.com\n");
writeFileSync("../../outside/models.ts", "export {};\n");
