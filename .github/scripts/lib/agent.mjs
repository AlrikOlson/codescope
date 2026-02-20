import { query } from "@anthropic-ai/claude-agent-sdk";
import { existsSync } from "node:fs";
import { join } from "node:path";

const DEFAULT_MODEL = "claude-sonnet-4-6";
const DEFAULT_MAX_TURNS = 10;

/**
 * Resolve the codescope-server binary path.
 * Checks CI build output first, then falls back to PATH.
 * @param {string} cwd - working directory
 * @returns {string}
 */
function resolveCodeScopeBinary(cwd) {
  // CI builds to server/target/release/codescope-server
  const ciBinary = join(cwd, "server", "target", "release", "codescope-server");
  if (existsSync(ciBinary)) return ciBinary;
  // Fall back to PATH
  return "codescope-server";
}

/**
 * CodeScope MCP server configuration for agent queries.
 * @param {string} cwd - working directory to index
 * @returns {object}
 */
export function codeScopeMcpConfig(cwd) {
  return {
    codescope: {
      command: resolveCodeScopeBinary(cwd),
      args: ["--mcp", "--root", cwd],
    },
  };
}

/**
 * Focused CodeScope tool set for CI agents.
 * Kept minimal to nudge the model toward semantic search as the primary
 * discovery tool. More tools = more decision paralysis = slower agents.
 *
 * - cs_search: unified discovery (PRIMARY — semantic + keyword fusion)
 * - cs_read: read specific files (supports mode=stubs for overviews)
 * - cs_grep: exact pattern matching (counting, specific strings)
 * - cs_status: verify indexed repo info
 *
 * @returns {string[]}
 */
export function codeScopeAllowedTools() {
  return [
    "mcp__codescope__cs_search",
    "mcp__codescope__cs_read",
    "mcp__codescope__cs_grep",
    "mcp__codescope__cs_status",
  ];
}

/**
 * Built-in tools to block for CodeScope-only agents.
 * MUST be comprehensive — with bypassPermissions, any tool not blocked is callable.
 * @returns {string[]}
 */
export function codeScopeOnlyDisallowedTools() {
  return [
    // Block sub-agent spawning and team tools
    "Task",
    "TaskCreate",
    "TaskUpdate",
    "TaskGet",
    "TaskList",
    "TaskOutput",
    "TaskStop",
    "TeamCreate",
    "TeamDelete",
    "SendMessage",
    // Block shell execution
    "Bash",
    // Block file writes
    "Write",
    "Edit",
    "NotebookEdit",
    // Block web access
    "WebSearch",
    "WebFetch",
    // Block interactive/mode tools
    "AskUserQuestion",
    "ExitPlanMode",
    "EnterPlanMode",
    "EnterWorktree",
    "Skill",
    // Block built-in read tools — force use of CodeScope MCP tools
    "Read",
    "Glob",
    "Grep",
  ];
}

/**
 * Run a Claude Agent SDK query with CodeScope MCP.
 * Streams messages, logs tool usage to stderr, returns the final text output.
 *
 * @param {{ prompt: string, systemPrompt: string, model?: string, maxTurns?: number, codeScopeOnly?: boolean }} params
 * @returns {Promise<string>}
 */
export async function runAgent({
  prompt,
  systemPrompt,
  model = DEFAULT_MODEL,
  maxTurns = DEFAULT_MAX_TURNS,
  codeScopeOnly = false,
}) {
  let lastText = "";

  const opts = {
    model,
    maxTurns,
    systemPrompt,
    mcpServers: codeScopeMcpConfig(process.cwd()),
    allowedTools: codeScopeAllowedTools(),
    permissionMode: "bypassPermissions",
    allowDangerouslySkipPermissions: true,
    cwd: process.cwd(),
  };

  if (codeScopeOnly) {
    opts.disallowedTools = codeScopeOnlyDisallowedTools();
  }

  for await (const message of query({
    prompt,
    options: opts,
  })) {
    if (message.type === "assistant" && message.message?.content) {
      for (const block of message.message.content) {
        if (block.type === "tool_use") {
          console.error(`[codescope] Using tool: ${block.name}`);
        }
        if (block.type === "text") {
          lastText = block.text;
          console.error(block.text);
        }
      }
    }

    if ("result" in message && message.subtype === "success") {
      lastText = message.result;
      console.error(`[result] ${message.result}`);
    } else if ("result" in message) {
      console.error(`[error] ${JSON.stringify(message)}`);
    }
  }

  return lastText;
}

/**
 * Parse structured JSON from agent output text.
 * Finds the last JSON object containing all required keys.
 *
 * @param {string} text - raw agent output
 * @param {string[]} requiredKeys - keys the JSON must contain
 * @returns {object|null}
 */
export function parseAgentJson(text, requiredKeys) {
  // Match all JSON objects in the text
  const jsonPattern = /\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\}/g;
  const matches = text.match(jsonPattern);
  if (!matches) return null;

  // Try from last match backwards — the final JSON is most likely the answer
  for (let i = matches.length - 1; i >= 0; i--) {
    try {
      const parsed = JSON.parse(matches[i]);
      const hasAllKeys = requiredKeys.every((key) => key in parsed);
      if (hasAllKeys) return parsed;
    } catch {
      continue;
    }
  }

  return null;
}
