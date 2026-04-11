import * as esbuild from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const outDir = join(__dirname, "src", "js");

const entries = [
  "pan_zoom",
  "search",
  "filter_engine",
  "group_toggle",
  "collapse",
  "highlight",
  "minimap",
  "shortcuts",
  "load_motion",
  "url_state",
];

await Promise.all(
  entries.map((name) =>
    esbuild.build({
      absWorkingDir: __dirname,
      entryPoints: [join(__dirname, "ts", `${name}.ts`)],
      bundle: true,
      format: "iife",
      platform: "browser",
      target: ["chrome120", "firefox120", "safari17"],
      outfile: join(outDir, `${name}.js`),
    }),
  ),
);
