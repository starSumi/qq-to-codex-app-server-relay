import { mkdtemp, mkdir, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

const qrConnect = vi.hoisted(() => vi.fn());

vi.mock("@tencent-connect/qqbot-connector", () => ({ qrConnect }));

import { resolveQQCredentials } from "../src/credentials.js";

const temporaryDirectories: string[] = [];

async function fixture(): Promise<{
  root: string;
  workspace: string;
  credentialFile: string;
}> {
  const root = await mkdtemp(path.join(os.tmpdir(), "qq-relay-credentials-"));
  temporaryDirectories.push(root);
  const workspace = path.join(root, "workspace");
  await mkdir(workspace);
  return {
    root,
    workspace,
    credentialFile: path.join(root, "state", "qqbot-credentials.json"),
  };
}

afterEach(async () => {
  qrConnect.mockReset();
  vi.restoreAllMocks();
  await Promise.all(
    temporaryDirectories
      .splice(0)
      .map((directory) => rm(directory, { force: true, recursive: true })),
  );
});

describe("resolveQQCredentials", () => {
  it("uses paired environment credentials without QR or disk access", async () => {
    const configured = { appId: "env-app", appSecret: "env-secret" };
    const statuses: string[] = [];
    await expect(
      resolveQQCredentials({
        configured,
        credentialFile: path.join("missing", "credentials.json"),
        workspace: path.join("missing", "workspace"),
        onStatus: (status) => statuses.push(status),
      }),
    ).resolves.toEqual(configured);
    expect(statuses).toEqual(["environment_loaded"]);
    expect(qrConnect).not.toHaveBeenCalled();
  });

  it("persists one QR account and reuses it on the next start", async () => {
    const { credentialFile, workspace } = await fixture();
    const statuses: string[] = [];
    qrConnect.mockResolvedValueOnce([
      { appId: "qr-app", appSecret: "qr-secret" },
    ]);

    const first = await resolveQQCredentials({
      configured: undefined,
      credentialFile,
      workspace,
      onStatus: (status) => statuses.push(status),
    });
    expect(first).toEqual({ appId: "qr-app", appSecret: "qr-secret" });
    expect(qrConnect).toHaveBeenCalledWith({
      source: "qq-to-codex-app-server-relay",
    });
    expect(JSON.parse(await readFile(credentialFile, "utf8"))).toEqual({
      version: 1,
      appId: "qr-app",
      appSecret: "qr-secret",
    });

    qrConnect.mockRejectedValueOnce(new Error("QR must not run again"));
    await expect(
      resolveQQCredentials({
        configured: undefined,
        credentialFile,
        workspace,
        onStatus: (status) => statuses.push(status),
      }),
    ).resolves.toEqual(first);
    expect(qrConnect).toHaveBeenCalledTimes(1);
    expect(statuses).toEqual(["qr_required", "qr_succeeded", "file_loaded"]);
  });

  it("rejects credential storage inside the Codex workspace", async () => {
    const { workspace } = await fixture();
    await expect(
      resolveQQCredentials({
        configured: undefined,
        credentialFile: path.join(workspace, "credentials.json"),
        workspace,
      }),
    ).rejects.toThrow(/must be outside CODEX_WORKSPACE/u);
    expect(qrConnect).not.toHaveBeenCalled();
  });

  it("reports QR failure through the status callback", async () => {
    const { credentialFile, workspace } = await fixture();
    const statuses: string[] = [];
    qrConnect.mockRejectedValueOnce(new Error("binding rejected"));

    await expect(
      resolveQQCredentials({
        configured: undefined,
        credentialFile,
        workspace,
        onStatus: (status) => statuses.push(status),
      }),
    ).rejects.toThrow("binding rejected");
    expect(statuses).toEqual(["qr_required", "qr_failed"]);
  });
});
