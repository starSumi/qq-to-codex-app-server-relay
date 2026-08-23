import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Ajv2020 } from "ajv/dist/2020.js";
import type { ValidateFunction } from "ajv";

import type { AdapterToCore, CoreToAdapter } from "./generated/relay-wire.js";

export const PROTOCOL_VERSION = "1";

const repositoryRoot = path.resolve(
  fileURLToPath(new URL("../../../", import.meta.url)),
);
const schemaText = readFileSync(
  path.join(repositoryRoot, "schemas", "relay-wire.schema.json"),
  "utf8",
);
const schema = JSON.parse(schemaText) as { readonly $id: string };
const ajv = new Ajv2020({ allErrors: true, strict: true });
ajv.addFormat("uint64", {
  type: "number",
  validate: (value: number) => Number.isSafeInteger(value) && value >= 0,
});
ajv.addSchema(schema);

function requireValidator(reference: string): ValidateFunction {
  const validator = ajv.getSchema(reference);
  if (!validator) {
    throw new Error(`relay schema does not expose ${reference}`);
  }
  return validator;
}
const validateAdapter = requireValidator(`${schema.$id}#/$defs/AdapterToCore`);
const validateCore = requireValidator(`${schema.$id}#/$defs/CoreToAdapter`);

export const SCHEMA_SHA256 = createHash("sha256")
  .update(schemaText)
  .digest("hex");

export function assertAdapterMessage(
  value: unknown,
): asserts value is AdapterToCore {
  if (!validateAdapter(value)) {
    throw new Error(
      `adapter message violates relay schema: ${ajv.errorsText(validateAdapter.errors)}`,
    );
  }
}

export function assertCoreMessage(
  value: unknown,
): asserts value is CoreToAdapter {
  if (!validateCore(value)) {
    throw new Error(
      `core message violates relay schema: ${ajv.errorsText(validateCore.errors)}`,
    );
  }
}
