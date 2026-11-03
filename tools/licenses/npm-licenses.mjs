// Appends notices for the npm packages bundled into the app UI (runtime
// dependencies only, resolved from app/package-lock.json).
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const app = new URL("../../app/", import.meta.url).pathname;
const lock = JSON.parse(readFileSync(join(app, "package-lock.json"), "utf8"));
const out = ["", "=".repeat(80), "JavaScript packages in the user interface", "=".repeat(80), ""];
const entries = Object.entries(lock.packages)
  .filter(([path, info]) => path.startsWith("node_modules/") && !info.dev)
  .sort(([a], [b]) => a.localeCompare(b));
for (const [path, info] of entries) {
  const dir = join(app, path);
  const name = path.replace(/^.*node_modules\//, "");
  const file = existsSync(dir) && readdirSync(dir).find((f) => /^(license|licence|copying)(\.|$)/i.test(f));
  out.push("-".repeat(80), `${name} ${info.version} (${info.license ?? "see text"})`, "-".repeat(80));
  out.push(file ? readFileSync(join(dir, file), "utf8").trim() : `License: ${info.license ?? "unknown"}`, "");
}
process.stdout.write(out.join("\n") + "\n");
