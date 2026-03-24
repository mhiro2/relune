import * as esbuild from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const outDir = join(__dirname, "src", "js");

const entries = [
  "pan_zoom",
  "search",
  "type_filter",
  "group_toggle",
  "collapse",
  "highlight",
];

await Promise.all(
  entries.map((name) =>
    esbuild.build({
      absWorkingDir: __dirname,
      entryPoints: [join(__dirname, "ts", `${name}.ts`)],
      bundle: true,
      format: "iife",
      platform: "browser",
      target: "es2020",
      outfile: join(outDir, `${name}.js`),
    }),
  ),
);
