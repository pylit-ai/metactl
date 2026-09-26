/** Pi bridge for MetaCTL's session-bound skill-discovery MCP host.
 *
 * Configure METACTL_DISCOVERY_ARGS_JSON with a JSON argv array that fixes the
 * project and all optional provider settings. This extension never selects a
 * paid route or reads a credential itself.
 */
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

const MAX_REPLY_BYTES = 2 * 1024 * 1024;
// load has two 15s CLI calls; discover has 15s CLI + up to 10s ranking.
const TIMEOUT_MS = 35_000;
const ERROR = "Skill discovery unavailable for this session; inspect the host locally.";
type Reply = { jsonrpc: string; id: number; result?: unknown; error?: unknown };

class DiscoveryHost {
  private child: ChildProcessWithoutNullStreams | undefined;
  private buffer = Buffer.alloc(0);
  private pending = new Map<number, { resolve: (value: Reply) => void; reject: () => void; timer: ReturnType<typeof setTimeout> }>();
  private nextId = 1;
  private queue: Promise<unknown> = Promise.resolve();
  private poisoned = false;

  private fail(): void {
    if (this.poisoned) return;
    this.poisoned = true;
    const child = this.child;
    if (child) {
      // EOF lets the Rust wrapper reap Python and remove its temporary files.
      child.stdin.end();
      const timer = setTimeout(() => {
        try {
          if (process.platform !== "win32" && child.pid) process.kill(-child.pid, "SIGTERM");
          else child.kill();
          const hardKill = setTimeout(() => {
            try {
              if (process.platform !== "win32" && child.pid) process.kill(-child.pid, "SIGKILL");
              else child.kill("SIGKILL");
            } catch { /* Process group already exited. */ }
          }, 750);
          hardKill.unref();
        } catch { /* Already exited. */ }
      }, 1000);
      timer.unref();
      child.once("exit", () => clearTimeout(timer));
    }
    for (const item of this.pending.values()) {
      clearTimeout(item.timer);
      item.reject();
    }
    this.pending.clear();
  }

  private onData(chunk: Buffer): void {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    if (this.buffer.length > MAX_REPLY_BYTES) return this.fail();
    for (;;) {
      const end = this.buffer.indexOf(10);
      if (end < 0) break;
      const line = this.buffer.subarray(0, end);
      this.buffer = this.buffer.subarray(end + 1);
      let reply: Reply;
      try { reply = JSON.parse(line.toString("utf8")); }
      catch { return this.fail(); }
      if (reply?.jsonrpc !== "2.0" || typeof reply.id !== "number") return this.fail();
      const item = this.pending.get(reply.id);
      if (!item) return this.fail();
      this.pending.delete(reply.id);
      clearTimeout(item.timer);
      item.resolve(reply);
    }
  }

  private request(method: string, params: unknown, signal?: AbortSignal): Promise<Reply> {
    if (this.poisoned || !this.child || signal?.aborted) {
      this.fail();
      return Promise.reject(new Error(ERROR));
    }
    const id = this.nextId++;
    return new Promise<Reply>((resolve, reject) => {
      const abort = () => this.fail();
      const timer = setTimeout(() => this.fail(), TIMEOUT_MS);
      this.pending.set(id, {
        resolve: (value) => { signal?.removeEventListener("abort", abort); resolve(value); },
        reject: () => { signal?.removeEventListener("abort", abort); reject(new Error(ERROR)); },
        timer,
      });
      signal?.addEventListener("abort", abort, { once: true });
      const message = JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n";
      if (Buffer.byteLength(message) > 65_536) return this.fail();
      this.child!.stdin.write(message, (error) => { if (error) this.fail(); });
    });
  }

  private async start(signal?: AbortSignal): Promise<void> {
    if (this.child || this.poisoned) return;
    const command = process.env.METACTL_DISCOVERY_COMMAND || "metactl";
    const rawArgs = process.env.METACTL_DISCOVERY_ARGS_JSON;
    let args: unknown;
    try { args = JSON.parse(rawArgs || "null"); }
    catch { this.fail(); throw new Error(ERROR); }
    if (!Array.isArray(args) || !args.every((arg) => typeof arg === "string") || args.length === 0) {
      this.fail(); throw new Error(ERROR);
    }
    this.child = spawn(command, args, { stdio: ["pipe", "pipe", "pipe"], shell: false,
      detached: process.platform !== "win32" });
    this.child.stdout.on("data", (chunk: Buffer) => this.onData(chunk));
    this.child.stderr.resume(); // Never echo host diagnostics or environment.
    this.child.stdin.on("error", () => this.fail());
    this.child.on("error", () => this.fail());
    this.child.on("exit", () => this.fail());
    const reply = await this.request("initialize", {
      protocolVersion: "2025-11-25", capabilities: {},
      clientInfo: { name: "metactl-pi-discovery", version: "0.1.0" },
    }, signal);
    if (reply.error || !reply.result) { this.fail(); throw new Error(ERROR); }
  }

  call(name: "discover_skills" | "load_skill", args: Record<string, string>, signal?: AbortSignal): Promise<{ content: Array<{ type: "text"; text: string }>; details: Record<string, never> }> {
    if (name === "discover_skills" && typeof args.query === "string" && Buffer.byteLength(args.query, "utf8") > 8192) {
      return Promise.reject(new Error("Discovery query exceeds 8192 UTF-8 bytes."));
    }
    const run = async () => {
      await this.start(signal);
      const reply = await this.request("tools/call", { name, arguments: args }, signal);
      const result = reply.result as { content?: unknown; isError?: unknown } | undefined;
      if (reply.error || !result || result.isError !== false || !Array.isArray(result.content) ||
          !result.content.every((part) => part?.type === "text" && typeof part.text === "string")) {
        throw new Error(ERROR);
      }
      return { content: result.content as Array<{ type: "text"; text: string }>, details: {} };
    };
    const result = this.queue.then(run);
    this.queue = result.catch(() => this.fail());
    return result.catch(() => { this.fail(); throw new Error(ERROR); });
  }

  close(): void { this.fail(); }
}

export default function (pi: ExtensionAPI): void {
  const host = new DiscoveryHost();
  pi.on("session_shutdown", () => host.close());
  pi.registerTool({
    name: "discover_skills",
    label: "Discover skills",
    description: "Find eligible MetaCTL specialist instructions for the current task or phase. No skill is activated by discovery.",
    parameters: Type.Object({ query: Type.String({ maxLength: 8192 }) }, { additionalProperties: false }),
    execute: (_id, params, signal) => host.call("discover_skills", { query: params.query }, signal),
  });
  pi.registerTool({
    name: "load_skill",
    label: "Load skill",
    description: "Load original instructions using an ID and digest returned by discover_skills.",
    parameters: Type.Object({
      id: Type.String({ pattern: "^[0-9a-f]{64}$" }),
      digest: Type.String({ pattern: "^[0-9a-f]{64}$" }),
    }, { additionalProperties: false }),
    execute: (_id, params, signal) => host.call("load_skill", { id: params.id, digest: params.digest }, signal),
  });
}
