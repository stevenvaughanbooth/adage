import { existsSync } from "node:fs";
import { registerHooks } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const PREFIX = "@harbor/dispatch/";
const SRC = join(dirname(fileURLToPath(import.meta.url)), "..", "src");

registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier.startsWith(PREFIX)) {
      const rel = specifier.slice(PREFIX.length);
      const file = join(SRC, rel.endsWith(".ts") ? rel : `${rel}.ts`);
      if (existsSync(file)) {
        return { url: pathToFileURL(file).href, shortCircuit: true };
      }
    }
    return nextResolve(specifier, context);
  },
});
