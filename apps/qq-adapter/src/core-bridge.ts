import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { once } from "node:events";
import { createInterface, type Interface } from "node:readline";
import type { Writable } from "node:stream";

import type {
  AdapterToCore,
  CoreToAdapter,
  InboundMessage,
} from "./generated/relay-wire.js";
import {
  assertAdapterMessage,
  assertCoreMessage,
  PROTOCOL_VERSION,
  SCHEMA_SHA256,
} from "./contract.js";
import { errorMessage } from "./logger.js";

type OutputHandler = (message: CoreToAdapter) => void | Promise<void>;
type FailureHandler = (error: Error) => void;

const MAX_PENDING_WRITES = 128;
const MAX_PENDING_OUTPUTS = 128;
const OUTPUT_PAUSE_THRESHOLD = 64;
const OUTPUT_RESUME_THRESHOLD = 32;

function waitForDrain(stream: Writable): Promise<void> {
  return new Promise((resolve, reject) => {
    const cleanup = (): void => {
      stream.off("drain", onDrain);
      stream.off("error", onError);
      stream.off("close", onClose);
    };
    const onDrain = (): void => {
      cleanup();
      resolve();
    };
    const onError = (error: Error): void => {
      cleanup();
      reject(error);
    };
    const onClose = (): void => {
      cleanup();
      reject(new Error("relay core stdin closed before draining"));
    };
    stream.once("drain", onDrain);
    stream.once("error", onError);
    stream.once("close", onClose);
  });
}

export class CoreBridge {
  readonly #coreBin: string;
  readonly #coreCwd: string;
  readonly #environment: NodeJS.ProcessEnv;
  readonly #logger: Pick<Console, "info" | "warn" | "error">;
  #child: ChildProcessWithoutNullStreams | undefined;
  #readline: Interface | undefined;
  #outputHandler: OutputHandler = () => undefined;
  #failureHandler: FailureHandler = () => undefined;
  #outputChain: Promise<void> = Promise.resolve();
  #writeChain: Promise<void> = Promise.resolve();
  #ready = false;
  #stopping = false;
  #failed = false;
  #pendingWrites = 0;
  #pendingOutputs = 0;
  #stdoutPaused = false;

  public constructor(
    coreBin: string,
    coreCwd: string,
    environment: NodeJS.ProcessEnv,
    logger: Pick<Console, "info" | "warn" | "error"> = console,
  ) {
    this.#coreBin = coreBin;
    this.#coreCwd = coreCwd;
    this.#environment = environment;
    this.#logger = logger;
  }

  public onOutput(handler: OutputHandler): void {
    this.#outputHandler = handler;
  }

  public onFailure(handler: FailureHandler): void {
    this.#failureHandler = handler;
  }

  public async start(): Promise<void> {
    if (this.#child) throw new Error("relay core is already started");
    this.#failed = false;
    this.#stopping = false;
    const child = spawn(this.#coreBin, [], {
      cwd: this.#coreCwd,
      env: this.#environment,
      shell: false,
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    this.#child = child;
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string) => {
      const line = chunk.trim();
      if (line) this.#logger.warn(`[core] ${line.slice(0, 4_000)}`);
    });
    this.#readline = createInterface({
      input: child.stdout,
      crlfDelay: Infinity,
    });

    let resolveReady: (() => void) | undefined;
    let rejectReady: ((error: Error) => void) | undefined;
    const ready = new Promise<void>((resolve, reject) => {
      resolveReady = resolve;
      rejectReady = reject;
    });
    const timer = setTimeout(
      () => rejectReady?.(new Error("relay core handshake timed out")),
      20_000,
    );

    this.#readline.on("line", (line) => {
      if (line.length > 1_048_576) {
        rejectReady?.(
          new Error("relay core emitted an oversized protocol line"),
        );
        this.#fail(new Error("relay core emitted an oversized protocol line"));
        return;
      }
      try {
        const message: unknown = JSON.parse(line);
        assertCoreMessage(message);
        if (message.type === "ready") {
          if (
            message.protocol_version !== PROTOCOL_VERSION ||
            message.schema_sha256 !== SCHEMA_SHA256
          ) {
            rejectReady?.(new Error("relay core schema handshake mismatch"));
            return;
          }
          this.#ready = true;
          resolveReady?.();
        } else if (message.type === "fatal") {
          const error = new Error(`relay core fatal: ${message.code}`);
          rejectReady?.(error);
          this.#fail(error);
        }
        this.#enqueueOutput(message);
      } catch (error) {
        const failure = new Error(
          `invalid relay core output: ${errorMessage(error)}`,
        );
        rejectReady?.(failure);
        this.#fail(failure);
      }
    });
    child.once("error", (error) => {
      rejectReady?.(error);
      this.#fail(error);
    });
    child.once("exit", (code, signal) => {
      if (!this.#stopping) {
        const error = new Error(
          `relay core exited unexpectedly (code=${String(code)}, signal=${String(signal)})`,
        );
        rejectReady?.(error);
        this.#fail(error);
      }
    });

    await this.#send({
      type: "hello",
      protocol_version: PROTOCOL_VERSION,
      schema_sha256: SCHEMA_SHA256,
      adapter: { name: "qq-adapter", version: "0.2.0" },
    });
    try {
      await ready;
    } finally {
      clearTimeout(timer);
    }
  }

  public async activate(): Promise<void> {
    this.#requireReady();
    await this.#send({ type: "activate", protocol_version: PROTOCOL_VERSION });
  }

  public async sendInbound(message: InboundMessage): Promise<void> {
    this.#requireReady();
    await this.#send({
      type: "inbound",
      protocol_version: PROTOCOL_VERSION,
      message,
    });
  }

  public async acknowledge(outboxId: string): Promise<void> {
    this.#requireReady();
    await this.#send({
      type: "delivery_ack",
      protocol_version: PROTOCOL_VERSION,
      outbox_id: outboxId,
    });
  }

  public async stop(): Promise<void> {
    if (!this.#child) return;
    this.#stopping = true;
    if (this.#child.stdin.writable && this.#ready) {
      await this.#send({
        type: "shutdown",
        protocol_version: PROTOCOL_VERSION,
      }).catch(() => undefined);
    }
    this.#child.stdin.end();
    const child = this.#child;
    let killTimer: NodeJS.Timeout | undefined;
    try {
      if (child.exitCode === null && child.signalCode === null) {
        await Promise.race([
          once(child, "exit"),
          new Promise<void>((resolve) => {
            killTimer = setTimeout(() => {
              child.kill();
              resolve();
            }, 5_000);
          }),
        ]);
      }
    } finally {
      if (killTimer) clearTimeout(killTimer);
    }
    this.#readline?.close();
    this.#child = undefined;
    this.#ready = false;
  }

  #requireReady(): void {
    if (!this.#ready) throw new Error("relay core is not ready");
  }

  #send(message: AdapterToCore): Promise<void> {
    assertAdapterMessage(message);
    if (this.#pendingWrites >= MAX_PENDING_WRITES) {
      const failure = new Error("relay core write queue is full");
      this.#fail(failure);
      return Promise.reject(failure);
    }
    this.#pendingWrites += 1;
    const write = this.#writeChain.then(async () => {
      const child = this.#child;
      if (!child?.stdin.writable)
        throw new Error("relay core stdin is not writable");
      if (!child.stdin.write(`${JSON.stringify(message)}\n`, "utf8")) {
        await waitForDrain(child.stdin);
      }
    });
    this.#writeChain = write.catch(() => undefined);
    const tracked = write.finally(() => {
      this.#pendingWrites -= 1;
    });
    void tracked.catch((error: unknown) => {
      this.#fail(new Error(`relay core write failed: ${errorMessage(error)}`));
    });
    return tracked;
  }

  #enqueueOutput(message: CoreToAdapter): void {
    if (this.#pendingOutputs >= MAX_PENDING_OUTPUTS) {
      this.#child?.stdout.pause();
      this.#stdoutPaused = true;
      this.#fail(new Error("relay core output queue is full"));
      return;
    }
    this.#pendingOutputs += 1;
    if (!this.#stdoutPaused && this.#pendingOutputs >= OUTPUT_PAUSE_THRESHOLD) {
      this.#child?.stdout.pause();
      this.#stdoutPaused = true;
    }
    this.#outputChain = this.#outputChain
      .then(() => this.#outputHandler(message))
      .catch((error: unknown) => {
        const failure = new Error(
          `relay output handler failed: ${errorMessage(error)}`,
        );
        this.#logger.error(failure.message);
        this.#fail(failure);
      })
      .finally(() => {
        this.#pendingOutputs -= 1;
        if (
          this.#stdoutPaused &&
          this.#pendingOutputs <= OUTPUT_RESUME_THRESHOLD &&
          !this.#failed
        ) {
          this.#child?.stdout.resume();
          this.#stdoutPaused = false;
        }
      });
  }

  #fail(error: Error): void {
    if (this.#failed || this.#stopping) return;
    this.#failed = true;
    this.#failureHandler(error);
  }
}
