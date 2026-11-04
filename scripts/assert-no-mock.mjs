#!/usr/bin/env node
// Fails the production build if the dev preview leaked into dist/ (SPEC §10,
// frontend-ipc-petdex-issues #11): dev.html must not be emitted, and no built
// file may contain the sentinel string exported by src/dev/mockBackend.ts.
//
// Usage: node scripts/assert-no-mock.mjs [distDir]   (default: ./dist)
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const SENTINEL = ["__FOCUS", "DEV", "MOCK__"].join("_");
const dist = resolve(process.argv[2] ?? "dist");

if (!existsSync(dist) || !statSync(dist).isDirectory()) {
  console.error(`assert-no-mock: ${dist} does not exist; run vite build first.`);
  process.exit(1);
}

const problems = [];
const walk = (dir) => {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(path);
      continue;
    }
    const rel = relative(dist, path);
    if (entry.name === "dev.html") problems.push(`${rel}: the dev preview page must not be built`);
    if (readFileSync(path).includes(SENTINEL)) problems.push(`${rel}: contains the dev mock sentinel`);
  }
};
walk(dist);

if (problems.length > 0) {
  console.error("assert-no-mock: the dev mock backend leaked into the production build:");
  for (const p of problems) console.error(`  - ${p}`);
  process.exit(1);
}
console.log(`assert-no-mock: ok (${relative(process.cwd(), dist) || "."} has no dev page or mock sentinel)`);
