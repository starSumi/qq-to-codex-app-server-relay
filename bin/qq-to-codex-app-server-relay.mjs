#!/usr/bin/env node

import { access } from "node:fs/promises";
import { constants } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";

const packageRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const adapterEntry = path.join(
  packageRoot,
  "apps",
  "qq-adapter",
  "dist",
  "main.js",
);
const coreBinary = path.join(
  packageRoot,
  "target",
  "release",
  "relay-core.exe",
);

function printHelp() {
  console.log(`QQ to Codex app-server relay

Usage:
  qq-to-codex-app-server-relay [start]
  qq-to-codex-app-server-relay --version
  qq-to-codex-app-server-relay --help

The public npm artifact currently supports Windows x64 only.
The first start opens the official QQ QR binding flow when no credential
environment variables or local credential file are present.

Important environment variables:
  RELAY_STATE_DIR              persistent relay state root
  RELAY_AUTH_MODE               dangerous (default) or safe
  QQBOT_ALLOW_FROM              comma-separated safe-mode sender IDs
  CODEX_HOME                   dedicated Codex home
  CODEX_WORKSPACE              isolated Codex workspace
  CODEX_NETWORK_ACCESS         true (default) or false
  CODEX_ALLOW_EXTERNAL_TOOLS   true (default) or false`);
}

function defaultStateRoot() {
  const base =
    process.env.LOCALAPPDATA?.trim() ||
    path.join(os.homedir(), "AppData", "Local");
  return path.join(base, "qq-to-codex-app-server-relay");
}

async function assertReadable(file, label) {
  try {
    await access(file, constants.R_OK);
  } catch {
    throw new Error(
      `${label} is missing from this npm installation. Reinstall the package or use the source release build.`,
    );
  }
}

async function start() {
  if (process.platform !== "win32" || process.arch !== "x64") {
    throw new Error("This npm release supports Windows x64 only.");
  }
  await assertReadable(adapterEntry, "The TypeScript adapter build");
  await assertReadable(coreBinary, "The relay-core executable");

  const stateRoot = path.resolve(
    process.env.RELAY_STATE_DIR?.trim() || defaultStateRoot(),
  );
  const environment = {
    ...process.env,
    CODEX_WORKSPACE:
      process.env.CODEX_WORKSPACE?.trim() || path.join(stateRoot, "workspace"),
    RELAY_DB:
      process.env.RELAY_DB?.trim() || path.join(stateRoot, "relay.sqlite3"),
    QQBOT_CREDENTIALS_FILE:
      process.env.QQBOT_CREDENTIALS_FILE?.trim() ||
      path.join(stateRoot, "qqbot-credentials.json"),
    RELAY_CORE_BIN: coreBinary,
  };

  const child = spawn(
    process.execPath,
    ["--enable-source-maps", adapterEntry],
    {
      cwd: packageRoot,
      env: environment,
      stdio: "inherit",
      windowsHide: false,
    },
  );

  let forwarding = false;
  const forwardSignal = (signal) => {
    if (forwarding) return;
    forwarding = true;
    child.kill(signal);
  };
  process.once("SIGINT", () => forwardSignal("SIGINT"));
  process.once("SIGTERM", () => forwardSignal("SIGTERM"));

  return await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (typeof code === "number") resolve(code);
      else reject(new Error(`relay exited with signal ${signal ?? "unknown"}`));
    });
  });
}

const command = process.argv[2] || "start";
try {
  if (command === "--help" || command === "-h") {
    printHelp();
    process.exitCode = 0;
  } else if (command === "--version" || command === "-v") {
    const manifest = await import("../manifest.json", {
      with: { type: "json" },
    });
    console.log(manifest.default.version);
    process.exitCode = 0;
  } else if (command === "start") {
    process.exitCode = await start();
  } else {
    throw new Error(`Unknown command '${command}'. Use --help for usage.`);
  }
} catch (error) {
  console.error(
    `relay CLI error: ${error instanceof Error ? error.message : String(error)}`,
  );
  process.exitCode = 1;
}
