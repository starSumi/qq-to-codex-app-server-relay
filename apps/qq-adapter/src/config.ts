import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import type { QQCredentials } from "./credentials.js";
import type { RelayLogLevel } from "./logger.js";

export interface AdapterConfig {
  readonly qqCredentials: QQCredentials | undefined;
  readonly qqCredentialsFile: string;
  readonly codexWorkspace: string;
  readonly gatewayReadyTimeoutMs: number;
  readonly qqLogLevel: RelayLogLevel;
  readonly coreBin: string;
  readonly coreCwd: string;
  readonly coreEnvironment: NodeJS.ProcessEnv;
}

const SAFE_SYSTEM_ENVIRONMENT = new Set([
  "PATH",
  "PATHEXT",
  "SYSTEMROOT",
  "WINDIR",
  "COMSPEC",
  "TEMP",
  "TMP",
  "TMPDIR",
  "HOME",
  "USERPROFILE",
  "LOCALAPPDATA",
  "APPDATA",
  "HTTP_PROXY",
  "HTTPS_PROXY",
  "NO_PROXY",
  "SSL_CERT_FILE",
  "SSL_CERT_DIR",
  "LANG",
  "LC_ALL",
  "TERM",
  "NO_COLOR",
]);

const CORE_CONFIGURATION = new Set([
  "RELAY_AUTH_MODE",
  "QQBOT_ALLOW_FROM",
  "CODEX_BIN",
  "CODEX_HOME",
  "CODEX_WORKSPACE",
  "CODEX_MODEL",
  "CODEX_APPROVAL_POLICY",
  "CODEX_SANDBOX",
  "CODEX_NETWORK_ACCESS",
  "CODEX_ALLOW_EXTERNAL_TOOLS",
  "CODEX_TURN_TIMEOUT_MS",
  "RELAY_DB",
  "RELAY_MAX_INPUT_BYTES",
  "RELAY_MAX_OUTPUT_BYTES",
  "RELAY_MAX_CONCURRENT_TURNS",
  "RELAY_MAX_QUEUE",
  "RELAY_RATE_LIMIT_PER_MINUTE",
  "RELAY_RETENTION_DAYS",
  "RELAY_LOG",
]);

function environmentCredentials(
  environment: NodeJS.ProcessEnv,
): QQCredentials | undefined {
  const appId = environment["QQBOT_APP_ID"]?.trim();
  const appSecret = environment["QQBOT_APP_SECRET"]?.trim();
  if (Boolean(appId) !== Boolean(appSecret)) {
    throw new Error(
      "QQBOT_APP_ID and QQBOT_APP_SECRET must be provided together",
    );
  }
  return appId && appSecret ? { appId, appSecret } : undefined;
}

function boundedInteger(
  environment: NodeJS.ProcessEnv,
  name: string,
  fallback: number,
  minimum: number,
  maximum: number,
): number {
  const raw = environment[name]?.trim();
  const value = raw ? Number(raw) : fallback;
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum) {
    throw new Error(
      `${name} must be an integer between ${minimum} and ${maximum}`,
    );
  }
  return value;
}

function qqLogLevel(environment: NodeJS.ProcessEnv): RelayLogLevel {
  const value = environment["QQBOT_LOG_LEVEL"]?.trim().toLowerCase() || "info";
  if (
    value === "debug" ||
    value === "info" ||
    value === "warn" ||
    value === "error"
  ) {
    return value;
  }
  throw new Error("QQBOT_LOG_LEVEL must be one of debug, info, warn, or error");
}

export function buildCoreEnvironment(
  environment: NodeJS.ProcessEnv,
): NodeJS.ProcessEnv {
  const result: NodeJS.ProcessEnv = {};
  for (const [key, value] of Object.entries(environment)) {
    const upper = key.toUpperCase();
    if (
      value !== undefined &&
      (SAFE_SYSTEM_ENVIRONMENT.has(upper) || CORE_CONFIGURATION.has(upper))
    ) {
      result[key] = value;
    }
  }
  return result;
}

export function loadConfig(
  environment: NodeJS.ProcessEnv = process.env,
): AdapterConfig {
  const repositoryRoot = path.resolve(
    fileURLToPath(new URL("../../../", import.meta.url)),
  );
  const executable =
    process.platform === "win32" ? "relay-core.exe" : "relay-core";
  const codexHome =
    environment["CODEX_HOME"]?.trim() || path.join(os.homedir(), ".codex");
  const codexWorkspace =
    environment["CODEX_WORKSPACE"]?.trim() ||
    path.join(repositoryRoot, "workspace");
  fs.mkdirSync(codexWorkspace, { recursive: true });
  const coreEnvironment = buildCoreEnvironment({
    ...environment,
    CODEX_HOME: codexHome,
    CODEX_WORKSPACE: codexWorkspace,
  });
  return Object.freeze({
    qqCredentials: environmentCredentials(environment),
    qqCredentialsFile: path.resolve(
      repositoryRoot,
      environment["QQBOT_CREDENTIALS_FILE"]?.trim() ||
        path.join("state", "qqbot-credentials.json"),
    ),
    codexWorkspace,
    gatewayReadyTimeoutMs: boundedInteger(
      environment,
      "QQBOT_READY_TIMEOUT_MS",
      30_000,
      1_000,
      300_000,
    ),
    qqLogLevel: qqLogLevel(environment),
    coreBin:
      environment["RELAY_CORE_BIN"]?.trim() ||
      path.join(repositoryRoot, "target", "release", executable),
    coreCwd: repositoryRoot,
    coreEnvironment,
  });
}
