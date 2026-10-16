// The scorer's own resolver for `@harbor/dispatch/*`, so the explicit hidden-test run does not
// depend on any file the agent could have changed. Resolves against the working directory.
import { existsSync } from "node:fs";
import { registerHooks } from "node:module";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const PREFIX = "@harbor/dispatch/";
const SRC = join(process.cwd(), "src");

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
