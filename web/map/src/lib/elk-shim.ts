// elkjs's bundle contains its own worker, which takes over any worker it is loaded in (it checks
// for a missing `document`). Here it must run in-process inside our layout worker instead, so it
// is shown a `document` before it loads. Imported first by layout.worker.ts.
const g = globalThis as unknown as { document?: unknown };
if (typeof g.document === "undefined") g.document = {};
export {};
