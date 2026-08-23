import { mkdir, readFile, writeFile } from "node:fs/promises";
import process from "node:process";
import { fileURLToPath, URL } from "node:url";
import path from "node:path";
import { compileFromFile } from "json-schema-to-typescript";

const adapterRoot = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const repositoryRoot = path.resolve(adapterRoot, "../..");
const schemaPath = path.join(
  repositoryRoot,
  "schemas",
  "relay-wire.schema.json",
);
const outputPath = path.join(adapterRoot, "src", "generated", "relay-wire.ts");
const mode = process.argv[2];

if (mode !== "--write" && mode !== "--check") {
  throw new Error("usage: generate-contract.mjs (--write|--check)");
}

const generated = await compileFromFile(schemaPath, {
  bannerComment:
    "/* Generated from Rust relay-protocol. Do not edit; run `pnpm schema:generate`. */",
  style: {
    bracketSpacing: true,
    printWidth: 100,
    semi: true,
    singleQuote: false,
    tabWidth: 2,
    trailingComma: "all",
    useTabs: false,
  },
  unreachableDefinitions: true,
});

if (mode === "--write") {
  await mkdir(path.dirname(outputPath), { recursive: true });
  await writeFile(outputPath, generated, "utf8");
} else {
  const current = await readFile(outputPath, "utf8");
  if (current !== generated) {
    throw new Error(
      "generated relay contract is stale; run `pnpm schema:generate`",
    );
  }
}
