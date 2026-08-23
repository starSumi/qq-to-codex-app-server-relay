import { afterEach, describe, expect, it, vi } from "vitest";

import {
  createRedactingLogger,
  errorMessage,
  redactLogMessage,
} from "../src/logger.js";

afterEach(() => vi.restoreAllMocks());

describe("redactLogMessage", () => {
  it("removes configured secrets and common token assignments", () => {
    const output = redactLogMessage(
      'secret-value Authorization: Bearer token-value "access_token":"second-token"',
      ["secret-value"],
    );
    expect(output).not.toContain("secret-value");
    expect(output).not.toContain("token-value");
    expect(output).not.toContain("second-token");
  });

  it("removes absolute local paths", () => {
    const windowsPath = ["E:", "\\private\\relay\\config.toml"].join("");
    const posixPath = ["", "home", "alice", ".codex", "config.toml"].join("/");
    const output = redactLogMessage(
      `failed at ${windowsPath} and ${posixPath}`,
      [],
    );

    expect(output).toBe("failed at [PATH] and [PATH]");
  });

  it("removes QQ gateway payloads while preserving the event type", () => {
    const output = redactLogMessage(
      '[qq-relay] Dispatch event: t=C2C_MESSAGE_CREATE payload={"author":{"user_openid":"sender-openid"},"content":"private prompt","id":"message-id"}',
      [],
    );

    expect(output).toBe(
      "[qq-relay] Dispatch event: t=C2C_MESSAGE_CREATE payload=[REDACTED]",
    );
    expect(output).not.toContain("sender-openid");
    expect(output).not.toContain("private prompt");
    expect(output).not.toContain("message-id");
  });

  it("removes QQ API bodies, resource identifiers, and trace identifiers", () => {
    const request = redactLogMessage(
      "[qqbot:api] >>> POST https://api.sgroup.qq.com/v2/users/sender-openid/messages (timeout: 30000ms)",
      [],
    );
    const requestBody = redactLogMessage(
      '[qqbot:api] >>> Body: {"content":"private reply","msg_id":"message-id"}',
      [],
    );
    const response = redactLogMessage(
      "[qqbot:api] <<< Status: 200 OK | TraceId: request-trace",
      [],
    );
    const responseBody = redactLogMessage(
      '[qqbot:api] <<< Body: {"id":"reply-id"}',
      [],
    );

    expect(request).toBe(
      "[qqbot:api] >>> POST https://api.sgroup.qq.com/v2/users/[REDACTED]/messages (timeout: 30000ms)",
    );
    expect(requestBody).toBe("[qqbot:api] >>> Body: [REDACTED]");
    expect(response).toBe(
      "[qqbot:api] <<< Status: 200 OK | TraceId: [REDACTED]",
    );
    expect(responseBody).toBe("[qqbot:api] <<< Body: [REDACTED]");
  });

  it("removes sensitive JSON fields outside known QQ SDK payload lines", () => {
    const output = redactLogMessage(
      'gateway failure {"content":"private prompt","user_openid":"sender-openid","group_openid":"group-id","channel_id":"channel-id","guild_id":"guild-id","session_id":"session-id","nickname":"alice"}',
      [],
    );

    expect(output).toBe(
      'gateway failure {"content":"[REDACTED]","user_openid":"[REDACTED]","group_openid":"[REDACTED]","channel_id":"[REDACTED]","guild_id":"[REDACTED]","session_id":"[REDACTED]","nickname":"[REDACTED]"}',
    );
  });

  it("suppresses SDK debug logs by default and redacts explicitly enabled debug logs", () => {
    const debug = vi
      .spyOn(console, "debug")
      .mockImplementation(() => undefined);
    const message =
      '[qq-relay] Dispatch event: t=C2C_MESSAGE_CREATE payload={"content":"private prompt"}';

    createRedactingLogger([]).debug(message);
    expect(debug).not.toHaveBeenCalled();

    createRedactingLogger([], "debug").debug(message);
    expect(debug).toHaveBeenCalledWith(
      "[qq-relay] Dispatch event: t=C2C_MESSAGE_CREATE payload=[REDACTED]",
    );
  });
});

describe("errorMessage", () => {
  it("keeps actionable error text for the redacting logger", () => {
    expect(
      errorMessage(
        new Error("Missing required environment variable: CODEX_HOME"),
      ),
    ).toBe("Missing required environment variable: CODEX_HOME");
  });
});
