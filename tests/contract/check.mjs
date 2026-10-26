// Type-checks the contract fixtures, and any real scans passed as arguments,
// against the hand-written types in guest-js/index.ts.
//
//   node tests/contract/check.mjs [scan.json ...]
//
// `cargo test --test contract` pins the same fixtures to src/models.rs, so the
// two checks together hold the TypeScript types to the serde output.
//
// Each JSON file is pasted into a generated .ts file as an object literal with
// `satisfies`, so a missing field, an extra one (a casing mismatch is both) and
// a wrong type all fail. Importing the JSON instead would widen every string
// literal and skip the excess-property check.

import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../..");
const outDir = join(root, "target/contract");
const outFile = join(outDir, "check.ts");

const literal = (path) =>
  JSON.stringify(JSON.parse(readFileSync(path, "utf8")), null, 2);

const lines = [
  `import type * as T from "../../guest-js/index";`,
  ``,
  `// Accepts a list only if it names every member of the union \`U\`, and`,
  `// names the members it is missing if not.`,
  `function variants<U>() {`,
  `  return <const L extends readonly U[]>(`,
  `    list: L & ([Exclude<U, L[number]>] extends [never]`,
  `      ? unknown`,
  `      : { missing: Exclude<U, L[number]> }),`,
  `  ) => list;`,
  `}`,
];

// Each file is named after the exported type it holds; enums.json maps union
// type names to every value serde produces for them.
for (const file of readdirSync(here).filter((f) => f.endsWith(".json")).sort()) {
  const path = join(here, file);
  lines.push(``, `// tests/contract/${file}`);
  if (file === "enums.json") {
    for (const [type, values] of Object.entries(JSON.parse(readFileSync(path, "utf8")))) {
      lines.push(`export const ${type} = variants<T.${type}>()(${JSON.stringify(values)});`);
    }
  } else {
    const type = basename(file, ".json");
    lines.push(`export const fixture${type} = ${literal(path)} satisfies T.${type};`);
  }
}

process.argv.slice(2).forEach((path, i) => {
  lines.push(``, `// ${path}`, `export const scan${i} = ${literal(path)} satisfies T.SystemInfo;`);
});

mkdirSync(outDir, { recursive: true });
writeFileSync(outFile, lines.join("\n") + "\n");

// Mirrors tsconfig.json.
const program = ts.createProgram([outFile], {
  target: ts.ScriptTarget.ES2021,
  module: ts.ModuleKind.ESNext,
  moduleResolution: ts.ModuleResolutionKind.Bundler,
  strict: true,
  skipLibCheck: true,
  noEmit: true,
});
const diagnostics = ts.getPreEmitDiagnostics(program);

if (diagnostics.length > 0) {
  console.error(
    ts.formatDiagnosticsWithColorAndContext(diagnostics, {
      getCanonicalFileName: (f) => f,
      getCurrentDirectory: () => root,
      getNewLine: () => "\n",
    }),
  );
  console.error(
    "guest-js/index.ts no longer matches the serde output of src/models.rs.",
  );
  process.exit(1);
}

const scans = process.argv.length - 2;
console.log(`guest-js/index.ts matches the contract fixtures and ${scans} real scan(s).`);
