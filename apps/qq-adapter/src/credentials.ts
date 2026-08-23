import { constants } from "node:fs";
import {
  chmod,
  lstat,
  mkdir,
  readFile,
  realpath,
  rename,
  unlink,
  writeFile,
} from "node:fs/promises";
import path from "node:path";

import { qrConnect } from "@tencent-connect/qqbot-connector";

export interface QQCredentials {
  readonly appId: string;
  readonly appSecret: string;
}

export type QQCredentialStatus =
  | "environment_loaded"
  | "file_loaded"
  | "qr_required"
  | "qr_failed"
  | "qr_succeeded";

interface CredentialFile {
  readonly version: 1;
  readonly appId: string;
  readonly appSecret: string;
}

function validateCredentials(value: unknown, source: string): QQCredentials {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${source} does not contain a credential object`);
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record).sort();
  if (
    keys.length !== 3 ||
    keys[0] !== "appId" ||
    keys[1] !== "appSecret" ||
    keys[2] !== "version" ||
    record["version"] !== 1
  ) {
    throw new Error(`${source} has an unsupported credential schema`);
  }
  const appId = record["appId"];
  const appSecret = record["appSecret"];
  if (
    typeof appId !== "string" ||
    typeof appSecret !== "string" ||
    !appId.trim() ||
    !appSecret.trim() ||
    appId.length > 256 ||
    appSecret.length > 1_024 ||
    /[\u0000-\u001f\u007f]/u.test(appId) ||
    /[\u0000-\u001f\u007f]/u.test(appSecret)
  ) {
    throw new Error(`${source} contains invalid QQ Bot credentials`);
  }
  return { appId: appId.trim(), appSecret: appSecret.trim() };
}

async function readCredentials(
  file: string,
): Promise<QQCredentials | undefined> {
  try {
    return validateCredentials(
      JSON.parse(await readFile(file, "utf8")) as unknown,
      "QQ Bot credential file",
    );
  } catch (error) {
    if (
      error &&
      typeof error === "object" &&
      "code" in error &&
      error.code === "ENOENT"
    ) {
      return undefined;
    }
    throw error;
  }
}

function isInside(candidate: string, root: string): boolean {
  const relative = path.relative(root, candidate);
  return (
    relative === "" ||
    (!relative.startsWith("..") && !path.isAbsolute(relative))
  );
}

async function assertCredentialLocation(
  credentialFile: string,
  workspace: string,
): Promise<void> {
  const parent = path.dirname(credentialFile);
  await mkdir(parent, { recursive: true });
  const [resolvedParent, resolvedWorkspace] = await Promise.all([
    realpath(parent),
    realpath(workspace),
  ]);
  if (isInside(resolvedParent, resolvedWorkspace)) {
    throw new Error("QQBOT_CREDENTIALS_FILE must be outside CODEX_WORKSPACE");
  }
  try {
    if ((await lstat(credentialFile)).isSymbolicLink()) {
      throw new Error("QQBOT_CREDENTIALS_FILE must not be a symbolic link");
    }
  } catch (error) {
    if (
      !error ||
      typeof error !== "object" ||
      !("code" in error) ||
      error.code !== "ENOENT"
    ) {
      throw error;
    }
  }
}

async function persistCredentials(
  credentialFile: string,
  credentials: QQCredentials,
): Promise<void> {
  const temporary = `${credentialFile}.tmp-${process.pid}-${Date.now()}`;
  const value: CredentialFile = { version: 1, ...credentials };
  try {
    await writeFile(temporary, `${JSON.stringify(value, undefined, 2)}\n`, {
      encoding: "utf8",
      flag: "wx",
      mode: 0o600,
    });
    await rename(temporary, credentialFile);
    await chmod(credentialFile, constants.S_IRUSR | constants.S_IWUSR);
  } catch (error) {
    await unlink(temporary).catch(() => undefined);
    throw error;
  }
}

export async function resolveQQCredentials(params: {
  readonly configured: QQCredentials | undefined;
  readonly credentialFile: string;
  readonly workspace: string;
  readonly onStatus?: (status: QQCredentialStatus) => void;
}): Promise<QQCredentials> {
  if (params.configured) {
    params.onStatus?.("environment_loaded");
    return params.configured;
  }

  await assertCredentialLocation(params.credentialFile, params.workspace);
  const stored = await readCredentials(params.credentialFile);
  if (stored) {
    params.onStatus?.("file_loaded");
    return stored;
  }

  params.onStatus?.("qr_required");
  try {
    const accounts = await qrConnect({
      source: "qq-to-codex-app-server-relay",
    });
    if (accounts.length !== 1) {
      throw new Error(
        `QQ QR binding returned ${accounts.length} accounts; expected exactly one`,
      );
    }
    const account = accounts[0];
    if (!account) throw new Error("QQ QR binding returned no account");
    const credentials = validateCredentials(
      { version: 1, appId: account.appId, appSecret: account.appSecret },
      "QQ QR binding",
    );
    await persistCredentials(params.credentialFile, credentials);
    params.onStatus?.("qr_succeeded");
    return credentials;
  } catch (error) {
    params.onStatus?.("qr_failed");
    throw error;
  }
}
