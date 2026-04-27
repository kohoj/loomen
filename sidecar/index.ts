import net from "node:net";
import os from "node:os";
import path from "node:path";
import fs from "node:fs";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";

type JsonRpcRequest = {
  jsonrpc: "2.0";
  id?: string | number;
  method: string;
  params?: Record<string, unknown>;
};

type JsonRpcResponse = {
  jsonrpc: "2.0";
  id: string | number | null;
  result?: unknown;
  error?: { code: number; message: string };
};

const socketPath = path.join(os.tmpdir(), `loomen-sidecar-${process.pid}.sock`);
const activeRuns = new Map<string, ChildProcessWithoutNullStreams>();
const canceledRuns = new Set<string>();
try {
  fs.unlinkSync(socketPath);
} catch {
  // Socket does not exist.
}

const server = net.createServer((socket) => {
  let buffer = "";
  socket.setEncoding("utf8");
  socket.on("data", (chunk) => {
    buffer += chunk;
    let newline = buffer.indexOf("\n");
    while (newline >= 0) {
      const line = buffer.slice(0, newline).trim();
      buffer = buffer.slice(newline + 1);
      if (line.length > 0) handleLine(socket, line);
      newline = buffer.indexOf("\n");
    }
  });
});

server.listen(socketPath, () => {
  process.stdout.write(`SOCKET_PATH=${socketPath}\n`);
});

function handleLine(socket: net.Socket, line: string) {
  let request: JsonRpcRequest;
  try {
    request = JSON.parse(line);
  } catch {
    write(socket, {
      jsonrpc: "2.0",
      id: null,
      error: { code: -32700, message: "Parse error" }
    });
    return;
  }

  if (!request || request.jsonrpc !== "2.0" || typeof request.method !== "string") {
    write(socket, {
      jsonrpc: "2.0",
      id: request?.id ?? null,
      error: { code: -32600, message: "Invalid Request" }
    });
    return;
  }

  void dispatch(socket, request).catch((error) => {
    if (request.id !== undefined) {
      write(socket, {
        jsonrpc: "2.0",
        id: request.id,
        error: { code: -32000, message: String(error?.message ?? error) }
      });
    }
  });
}

async function dispatch(socket: net.Socket, request: JsonRpcRequest) {
  switch (request.method) {
    case "query": {
      await runAgentQuery(socket, request);
      return;
    }
    case "cancel": {
      const id = String(request.params?.id ?? "");
      const child = activeRuns.get(id);
      if (child) {
        canceledRuns.add(id);
        child.kill("SIGTERM");
        activeRuns.delete(id);
      }
      return respond(socket, request, { ok: true });
    }
    case "claudeAuth": {
      const options = request.params?.options && typeof request.params.options === "object"
        ? request.params.options as Record<string, unknown>
        : {};
      const status = await runQuick(
        resolveBinary("claude", String(options.claudeExecutablePath ?? "")),
        ["auth", "status"],
        process.cwd(),
        10_000
      );
      return respond(socket, request, {
        id: request.params?.id,
        type: "claude_auth_output",
        agentType: "claude",
        accountInfo: status.ok ? status.output.trim() : null,
        error: status.ok ? null : status.output.trim() || "Claude auth status failed"
      });
    }
    case "workspaceInit":
      return respond(socket, request, {
        id: request.params?.id,
        type: "workspace_init_output",
        agentType: "claude",
        slashCommands: discoverSlashCommands(String(request.params?.options?.cwd ?? process.cwd())),
        mcpServers: discoverAgents(String(request.params?.options?.cwd ?? process.cwd())),
        error: null
      });
    case "contextUsage":
      return respond(socket, request, {
        usedTokens: 0,
        maxTokens: 0,
        percent: 0
      });
    case "updatePermissionMode":
    case "resetGenerator":
      return;
    default:
      if (request.id !== undefined) {
        return write(socket, {
          jsonrpc: "2.0",
          id: request.id,
          error: { code: -32601, message: `Method not found: ${request.method}` }
        });
      }
  }
}

async function runAgentQuery(socket: net.Socket, request: JsonRpcRequest) {
  const params = request.params ?? {};
  const id = String(params.id ?? "unknown");
  const agentType = String(params.agentType ?? "claude") === "codex" ? "codex" : "claude";
  const prompt = String(params.prompt ?? "");
  const options = params.options && typeof params.options === "object" ? params.options as Record<string, unknown> : {};
  const cwd = String(options.cwd ?? process.cwd());
  const model = String(options.model ?? (agentType === "codex" ? "gpt-5-codex" : "opus"));
  const permissionMode = String(options.permissionMode ?? "default");
  const providerEnv = parseProviderEnv(String(options.providerEnv ?? ""));
  const commandEnv = { ...process.env, ...providerEnv };
  const claudeExecutablePath = String(options.claudeExecutablePath ?? "");
  const codexExecutablePath = String(options.codexExecutablePath ?? "");
  const codexEffort = String(options.codexEffort ?? "high");

  const { command, args } = agentType === "codex"
    ? codexCommand(prompt, cwd, model, codexEffort, codexExecutablePath)
    : claudeCommand(prompt, model, permissionMode, claudeExecutablePath);

  const child = spawn(command, args, {
    cwd,
    env: commandEnv,
    stdio: ["ignore", "pipe", "pipe"]
  });
  activeRuns.set(id, child);

  let stdout = "";
  let stderr = "";
  const streamedTexts: string[] = [];
  let stdoutBuffer = "";
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    stdout += chunk;
    stdoutBuffer += chunk;
    let newline = stdoutBuffer.indexOf("\n");
    while (newline >= 0) {
      const line = stdoutBuffer.slice(0, newline);
      stdoutBuffer = stdoutBuffer.slice(newline + 1);
      const parsed = parseJsonLine(line);
      if (parsed && shouldForwardSessionEvent(parsed)) {
        notify(socket, "sessionEventNotification", {
          sessionId: id,
          agentType,
          event: parsed
        });
      }
      for (const text of extractAgentEventTexts(agentType, line)) {
        streamedTexts.push(text);
        notifyAgentMessage(socket, id, text);
      }
      newline = stdoutBuffer.indexOf("\n");
    }
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });

  const exitCode = await new Promise<number | null>((resolve) => {
    child.on("close", (code) => resolve(code));
    child.on("error", () => resolve(null));
  });
  activeRuns.delete(id);
  const wasCanceled = canceledRuns.delete(id);
  if (stdoutBuffer.trim()) {
    const parsed = parseJsonLine(stdoutBuffer);
    if (parsed && shouldForwardSessionEvent(parsed)) {
      notify(socket, "sessionEventNotification", {
        sessionId: id,
        agentType,
        event: parsed
      });
    }
    for (const text of extractAgentEventTexts(agentType, stdoutBuffer)) {
      streamedTexts.push(text);
      notifyAgentMessage(socket, id, text);
    }
  }

  const text = extractAgentText(agentType, stdout) || stdout.trim();
  const finalText = exitCode === 0 && text
    ? text
    : [
        `[${agentType} exited ${exitCode ?? "without status"}]`,
        text,
        stderr.trim()
      ].filter(Boolean).join("\n\n");

  let resultText = dedupeAdjacent(streamedTexts).join("\n").trim()
    || finalText
    || `[${agentType}] completed without text output.`;
  if (wasCanceled) {
    resultText = [resultText, `[${agentType} canceled]`].filter(Boolean).join("\n\n");
  }
  if (streamedTexts.length === 0) {
    notifyAgentMessage(socket, id, resultText);
  }
  if (exitCode === 0 && !wasCanceled) {
    respond(socket, request, { ok: true, text: resultText, exitCode });
  } else {
    notify(socket, "queryError", {
      sessionId: id,
      type: "query_error",
      error: resultText,
      exitCode
    });
    respond(socket, request, { ok: false, text: resultText, exitCode });
  }
}

function claudeCommand(prompt: string, model: string, permissionMode: string, executablePath = "") {
  const args = [
    "-p",
    prompt,
    "--output-format",
    "stream-json",
    "--model",
    model,
    "--permission-mode",
    normalizeClaudePermissionMode(permissionMode)
  ];
  return { command: resolveBinary("claude", executablePath), args };
}

function codexCommand(prompt: string, cwd: string, model: string, effort: string, executablePath = "") {
  return {
    command: resolveBinary("codex", executablePath),
    args: [
      "exec",
      "--json",
      "--color",
      "never",
      "-C",
      cwd,
      "-m",
      model,
      "-c",
      `model_reasoning_effort=${normalizeCodexEffort(effort)}`,
      "-s",
      "workspace-write",
      "-a",
      "never",
      prompt
    ]
  };
}

function normalizeClaudePermissionMode(value: string) {
  const allowed = new Set(["acceptEdits", "auto", "bypassPermissions", "default", "dontAsk", "plan"]);
  return allowed.has(value) ? value : "default";
}

function normalizeCodexEffort(value: string) {
  const allowed = new Set(["minimal", "low", "medium", "high", "xhigh"]);
  return allowed.has(value) ? value : "high";
}

function resolveBinary(name: "claude" | "codex", explicitPath = "") {
  if (explicitPath && fs.existsSync(explicitPath)) return explicitPath;
  const override = process.env[`LOOMEN_${name.toUpperCase()}_BIN`];
  if (override && fs.existsSync(override)) return override;
  return name;
}

function parseProviderEnv(text: string) {
  const env: Record<string, string> = {};
  for (const rawLine of text.split(/\r?\n/)) {
    let line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    if (line.startsWith("export ")) line = line.slice("export ".length).trim();
    const index = line.indexOf("=");
    if (index <= 0) continue;
    const key = line.slice(0, index).trim();
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) continue;
    let value = line.slice(index + 1).trim();
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
      value = value.slice(1, -1);
    }
    env[key] = value;
  }
  return env;
}

function notifyAgentMessage(socket: net.Socket, sessionId: string, text: string) {
  if (!text.trim()) return;
  notify(socket, "message", {
    sessionId,
    type: "assistant",
    message: {
      type: "assistant",
      role: "assistant",
      content: [{ type: "text", text }]
    }
  });
}

function extractAgentEventTexts(agentType: string, line: string) {
  const parsed = parseJsonLine(line);
  if (!parsed) return [];
  if (agentType === "claude") {
    return extractClaudeEventTexts(parsed, false);
  }
  const texts: string[] = [];
  collectLikelyText(parsed, texts);
  return dedupeAdjacent(texts);
}

function shouldForwardSessionEvent(parsed: any) {
  if (!parsed || typeof parsed !== "object") return false;
  if (typeof parsed.type === "string" && !["assistant", "result"].includes(parsed.type)) return true;
  if (Array.isArray(parsed.message?.content)) {
    return parsed.message.content.some((item: any) => item?.type && item.type !== "text");
  }
  return false;
}

function extractClaudeEventTexts(parsed: any, includeResult = true) {
  const texts: string[] = [];
  if (parsed.type === "assistant" && Array.isArray(parsed.message?.content)) {
    for (const item of parsed.message.content) {
      if (item?.type === "text" && typeof item.text === "string") texts.push(item.text);
    }
  } else if (parsed.type === "content_block_delta" && parsed.delta?.type === "text_delta") {
    if (typeof parsed.delta.text === "string") texts.push(parsed.delta.text);
  } else if (includeResult && parsed.type === "result" && typeof parsed.result === "string") {
    texts.push(parsed.result);
  }
  return dedupeAdjacent(texts);
}

function extractAgentText(agentType: string, stdout: string) {
  if (agentType === "claude") {
    const texts: string[] = [];
    for (const line of stdout.split(/\r?\n/)) {
      const parsed = parseJsonLine(line);
      if (!parsed) continue;
      texts.push(...extractClaudeEventTexts(parsed, true));
    }
    return dedupeAdjacent(texts).join("\n").trim();
  }

  const texts: string[] = [];
  for (const line of stdout.split(/\r?\n/)) {
    const parsed = parseJsonLine(line);
    if (!parsed) continue;
    collectLikelyText(parsed, texts);
  }
  return dedupeAdjacent(texts).join("\n").trim();
}

function parseJsonLine(line: string): any | null {
  const clean = line.trim();
  if (!clean.startsWith("{")) return null;
  try {
    return JSON.parse(clean);
  } catch {
    return null;
  }
}

function collectLikelyText(value: unknown, texts: string[]) {
  if (!value || typeof value !== "object") return;
  if (Array.isArray(value)) {
    for (const item of value) collectLikelyText(item, texts);
    return;
  }
  const record = value as Record<string, unknown>;
  for (const key of ["final", "message", "content", "text", "delta"]) {
    if (typeof record[key] === "string" && record[key].trim()) {
      texts.push(record[key] as string);
    }
  }
  for (const item of Object.values(record)) {
    if (item && typeof item === "object") collectLikelyText(item, texts);
  }
}

function dedupeAdjacent(values: string[]) {
  const result: string[] = [];
  for (const value of values.map((item) => item.trim()).filter(Boolean)) {
    if (result[result.length - 1] !== value) result.push(value);
  }
  return result;
}

function discoverSlashCommands(cwd: string) {
  const roots = [
    path.join(os.homedir(), ".claude", "commands"),
    path.join(cwd, ".claude", "commands")
  ];
  return roots.flatMap((root) => listMarkdownNames(root).map((name) => ({ name: `/${name}`, path: root })));
}

function discoverAgents(cwd: string) {
  const roots = [
    path.join(os.homedir(), ".claude", "agents"),
    path.join(cwd, ".claude", "agents")
  ];
  return roots.flatMap((root) => listMarkdownNames(root).map((name) => ({ name, path: root })));
}

function listMarkdownNames(root: string) {
  try {
    return fs.readdirSync(root)
      .filter((item) => item.endsWith(".md"))
      .map((item) => path.basename(item, ".md"))
      .sort();
  } catch {
    return [];
  }
}

function runQuick(command: string, args: string[], cwd: string, timeoutMs: number) {
  return new Promise<{ ok: boolean; output: string }>((resolve) => {
    const child = spawn(command, args, { cwd, env: process.env, stdio: ["ignore", "pipe", "pipe"] });
    let output = "";
    const timer = setTimeout(() => {
      child.kill("SIGTERM");
      resolve({ ok: false, output: "command timed out" });
    }, timeoutMs);
    child.stdout.on("data", (chunk) => { output += chunk; });
    child.stderr.on("data", (chunk) => { output += chunk; });
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ ok: code === 0, output });
    });
    child.on("error", (error) => {
      clearTimeout(timer);
      resolve({ ok: false, output: String(error?.message ?? error) });
    });
  });
}

function respond(socket: net.Socket, request: JsonRpcRequest, result: unknown) {
  if (request.id === undefined) return;
  write(socket, { jsonrpc: "2.0", id: request.id, result });
}

function notify(socket: net.Socket, method: string, params: unknown) {
  socket.write(`${JSON.stringify({ jsonrpc: "2.0", method, params })}\n`);
}

function write(socket: net.Socket, response: JsonRpcResponse) {
  socket.write(`${JSON.stringify(response)}\n`);
}

function shutdown() {
  server.close();
  try {
    fs.unlinkSync(socketPath);
  } catch {
    // Already gone.
  }
}

process.on("SIGINT", () => {
  shutdown();
  process.exit(130);
});
process.on("SIGTERM", () => {
  shutdown();
  process.exit(143);
});
