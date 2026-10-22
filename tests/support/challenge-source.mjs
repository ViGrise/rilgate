import { readFile } from "node:fs/promises";

const directory = new URL("../../crates/rilgate-web/generated/", import.meta.url);
const manifest = JSON.parse(await readFile(new URL("manifest.json", directory), "utf8"));
export const challengeSource = (await Promise.all(manifest.startup.map((member) =>
  readFile(new URL(member.output, directory), "utf8"),
))).join("\n;\n");
