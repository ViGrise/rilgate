import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import test from "node:test";

const locales = new URL("../web/i18n/", import.meta.url);
const english = JSON.parse(readFileSync(new URL("en.json", locales), "utf8"));
const englishKeys = Object.keys(english).sort();

function placeholders(value) {
  return [...new Set([...value.matchAll(/\{([^{}]+)\}/g)].map((match) => match[1]))].sort();
}

for (const filename of readdirSync(locales).filter((name) => name.endsWith(".json"))) {
  if (filename === "en.json") continue;
  const translations = JSON.parse(readFileSync(new URL(filename, locales), "utf8"));

  test(`${filename} has the English keys`, () => {
    const missing = englishKeys.filter((key) => !Object.hasOwn(translations, key));
    const extra = Object.keys(translations).filter((key) => !Object.hasOwn(english, key));
    assert.deepEqual(
      { missing, extra },
      { missing: [], extra: [] },
      `${filename}: missing keys: ${missing.join(", ")}; extra keys: ${extra.join(", ")}`,
    );
  });

  test(`${filename} preserves English placeholders`, () => {
    for (const [key, value] of Object.entries(translations)) {
      if (!Object.hasOwn(english, key)) continue;
      assert.deepEqual(placeholders(value), placeholders(english[key]), `${filename}: ${key}`);
    }
  });
}
