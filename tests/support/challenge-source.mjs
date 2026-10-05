import {readFile} from "node:fs/promises";
const files = [
  "js/challenge/runtime.js", "js/challenge/registry.js", "js/challenge/p2pool.js",
  "js/solvers/fast/solver.js", "js/solvers/fast/adapter.js", "js/solvers/randomx/adapter.js", "js/challenge.js",
];
export const challengeSource = (await Promise.all(files.map((file) =>
  readFile(new URL(`../../web/static/${file}`, import.meta.url), "utf8"),
))).map((source,index) => index === files.length - 1 ? "window.PowClient.ready = true;\n" + source : source).join("\n");
