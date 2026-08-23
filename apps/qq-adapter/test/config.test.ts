import { describe, expect, it } from "vitest";

import { buildCoreEnvironment, loadConfig } from "../src/config.js";

describe("buildCoreEnvironment", () => {
  it("keeps required runtime settings but strips QQ credentials and unrelated secrets", () => {
    const result = buildCoreEnvironment({
      PATH: "bin",
      QQBOT_APP_ID: "id",
      QQBOT_APP_SECRET: "secret",
      QQBOT_ALLOW_FROM: "openid",
      RELAY_AUTH_MODE: "safe",
      CODEX_HOME: "codex-home",
      CODEX_ALLOW_EXTERNAL_TOOLS: "false",
      OPENAI_API_KEY: "must-not-pass",
      UNRELATED_TOKEN: "must-not-pass",
    });
    expect(result).toEqual({
      PATH: "bin",
      RELAY_AUTH_MODE: "safe",
      QQBOT_ALLOW_FROM: "openid",
      CODEX_HOME: "codex-home",
      CODEX_ALLOW_EXTERNAL_TOOLS: "false",
    });
  });

  it("passes the QR-mode policy choice to the Rust policy owner", () => {
    const result = buildCoreEnvironment({
      RELAY_AUTH_MODE: "dangerous",
      QQBOT_APP_SECRET: "must-not-pass",
    });
    expect(result).toEqual({
      RELAY_AUTH_MODE: "dangerous",
    });
  });
});

describe("loadConfig", () => {
  it("accepts paired QQ credentials and rejects partial overrides", () => {
    const config = loadConfig({
      QQBOT_APP_ID: "id",
      QQBOT_APP_SECRET: "secret",
    });
    expect(config.qqCredentials).toEqual({ appId: "id", appSecret: "secret" });
    expect(() => loadConfig({ QQBOT_APP_ID: "id" })).toThrow(
      /must be provided together/u,
    );
  });

  it("bounds QQ gateway readiness timeout", () => {
    expect(() =>
      loadConfig({
        QQBOT_APP_ID: "id",
        QQBOT_APP_SECRET: "secret",
        QQBOT_READY_TIMEOUT_MS: "999",
      }),
    ).toThrow(/QQBOT_READY_TIMEOUT_MS/u);
    expect(
      loadConfig({
        QQBOT_APP_ID: "id",
        QQBOT_APP_SECRET: "secret",
        QQBOT_READY_TIMEOUT_MS: "45000",
      }).gatewayReadyTimeoutMs,
    ).toBe(45_000);
  });

  it("defaults QQ SDK logs to info and validates explicit levels", () => {
    expect(
      loadConfig({ QQBOT_APP_ID: "id", QQBOT_APP_SECRET: "secret" }).qqLogLevel,
    ).toBe("info");
    expect(
      loadConfig({
        QQBOT_APP_ID: "id",
        QQBOT_APP_SECRET: "secret",
        QQBOT_LOG_LEVEL: "debug",
      }).qqLogLevel,
    ).toBe("debug");
    expect(() =>
      loadConfig({
        QQBOT_APP_ID: "id",
        QQBOT_APP_SECRET: "secret",
        QQBOT_LOG_LEVEL: "verbose",
      }),
    ).toThrow(/QQBOT_LOG_LEVEL/u);
  });
});
