import * as esbuild from "esbuild";
import { cp, mkdir, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = dirname(__dirname);
const distDir = join(__dirname, "dist");
const assetsDir = join(distDir, "assets");
const examplesDir = join(distDir, "examples");

await mkdir(assetsDir, { recursive: true });
await mkdir(examplesDir, { recursive: true });

await esbuild.build({
  absWorkingDir: __dirname,
  entryPoints: [join(__dirname, "src", "main.ts")],
  bundle: true,
  format: "esm",
  outdir: assetsDir,
  platform: "browser",
  target: ["chrome120", "firefox120", "safari17"],
  external: ["../pkg/relune_wasm.js"],
});

await cp(join(__dirname, "index.html"), join(distDir, "index.html"));
await cp(join(__dirname, "src", "styles.css"), join(assetsDir, "styles.css"));
await cp(join(rootDir, "assets", "logo.png"), join(assetsDir, "logo.png"));
await cp(
  join(rootDir, "fixtures", "sql", "simple_blog.sql"),
  join(examplesDir, "simple_blog.sql"),
);
await cp(
  join(rootDir, "fixtures", "sql", "ecommerce.sql"),
  join(examplesDir, "ecommerce.sql"),
);
await cp(
  join(rootDir, "fixtures", "sql", "multi_schema.sql"),
  join(examplesDir, "multi_schema.sql"),
);
await writeFile(join(distDir, ".nojekyll"), "");
