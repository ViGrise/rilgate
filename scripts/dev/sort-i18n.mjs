#!/usr/bin/env node
import { readFile, writeFile, readdir, stat } from "node:fs/promises";
import { join } from "node:path";

async function sortJson(file) {
  const data = JSON.parse(await readFile(file, "utf8"));

  if (!data || Array.isArray(data) || typeof data !== "object") {
    throw new Error(`Expected JSON object: ${file}`);
  }

  const keys = Object.keys(data).sort();
  const lines = ["{"];

  for (let i = 0; i < keys.length; i++) {
    const key = keys[i];
    const section = key.split(".")[0];
    const previousSection = keys[i - 1]?.split(".")[0];

    if (i > 0 && section !== previousSection) {
      lines.push("");
    }

    const comma = i < keys.length - 1 ? "," : "";
    lines.push(`  ${JSON.stringify(key)}: ${JSON.stringify(data[key])}${comma}`);
  }

  lines.push("}", "");

  await writeFile(file, lines.join("\n"), "utf8");
  console.log(`Sorted: ${file}`);
}

async function main() {
  const inputs = process.argv.slice(2);

  if (!inputs.length) {
    inputs.push("web/i18n");
  }

  for (const input of inputs) {
    const info = await stat(input);

    if (info.isDirectory()) {
      const files = (await readdir(input)).filter((name) => name.endsWith(".json")).sort();

      for (const file of files) {
        await sortJson(join(input, file));
      }
    } else {
      await sortJson(input);
    }
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
