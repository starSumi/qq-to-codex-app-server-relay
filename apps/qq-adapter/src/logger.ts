import type { Logger } from "@tencent-connect/qqbot-nodejs";

export type RelayLogger = Required<Logger>;
export type RelayLogLevel = "debug" | "info" | "warn" | "error";

export function errorMessage(error: unknown): string {
  if (!(error instanceof Error)) return "unknown error";
  return error.message || error.name || "operation failed";
}

const SENSITIVE_ASSIGNMENT =
  /((?:authorization|access[_-]?token|app[_-]?secret|client[_-]?secret|token)["']?\s*[:=]\s*)(?:bearer\s+)?["']?[^"',;\s}]+["']?/giu;
const SDK_PAYLOAD = /(\bpayload=).*$/giu;
const SDK_BODY = /((?:>>>|<<<)\s+Body:\s*).*$/giu;
const SENSITIVE_JSON_VALUE =
  /("(?:content|id|message_id|msg_id|openid|union_openid|user_openid|group_openid|member_openid|group_member_openid|target_id|session_id|channel_id|guild_id|user_id|event_id|username|nickname|ref_idx)"\s*:\s*)(?:"(?:\\.|[^"\\])*"|null|true|false|-?\d+(?:\.\d+)?(?:e[+-]?\d+)?)/giu;
const SENSITIVE_RESOURCE_PATH =
  /(\/(?:users|groups|channels|guilds|dms)\/)[^/?\s]+/giu;
const TRACE_IDENTIFIER = /(\bTraceId:\s*)[^\s|]+/giu;
const SENSITIVE_LABEL_VALUE =
  /(\b(?:etag|file|file_uuid|md5|md5_10m|requestId|sha1|upload_id)\s*=\s*)[^\s,)]+/giu;
const WINDOWS_ABSOLUTE_PATH =
  /"[A-Z]:\\[^"\r\n]+"|\b[A-Z]:\\(?:[^\\\r\n:*?"<>|\s]+\\)*[^\\\r\n:*?"<>|\s]+/giu;
const POSIX_ABSOLUTE_PATH =
  /(?<!:)\/(?:Users|home|tmp|var|opt|srv|etc|mnt|workspace|workspaces)\/[^\s"'`;,)\]}]+/gu;

export function redactLogMessage(
  message: string,
  secrets: readonly string[],
): string {
  let result = message;
  for (const secret of secrets) {
    if (secret.length > 0) result = result.replaceAll(secret, "[REDACTED]");
  }
  result = result.replace(SDK_PAYLOAD, "$1[REDACTED]");
  result = result.replace(SDK_BODY, "$1[REDACTED]");
  result = result.replace(SENSITIVE_JSON_VALUE, '$1"[REDACTED]"');
  result = result.replace(SENSITIVE_ASSIGNMENT, "$1[REDACTED]");
  result = result.replace(SENSITIVE_RESOURCE_PATH, "$1[REDACTED]");
  result = result.replace(TRACE_IDENTIFIER, "$1[REDACTED]");
  result = result.replace(SENSITIVE_LABEL_VALUE, "$1[REDACTED]");
  result = result.replace(WINDOWS_ABSOLUTE_PATH, "[PATH]");
  result = result.replace(POSIX_ABSOLUTE_PATH, "[PATH]");
  return result.length <= 2_000
    ? result
    : `${result.slice(0, 2_000)} [truncated]`;
}

export function createRedactingLogger(
  secrets: readonly string[],
  minimumLevel: RelayLogLevel = "info",
): RelayLogger {
  const priority: Record<RelayLogLevel, number> = {
    debug: 10,
    info: 20,
    warn: 30,
    error: 40,
  };
  const enabled = (level: RelayLogLevel): boolean =>
    priority[level] >= priority[minimumLevel];
  return {
    info: (message) => {
      if (enabled("info")) console.info(redactLogMessage(message, secrets));
    },
    warn: (message) => {
      if (enabled("warn")) console.warn(redactLogMessage(message, secrets));
    },
    error: (message) => {
      if (enabled("error")) console.error(redactLogMessage(message, secrets));
    },
    debug: (message) => {
      if (enabled("debug")) console.debug(redactLogMessage(message, secrets));
    },
  };
}
