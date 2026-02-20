import { query } from "@anthropic-ai/claude-agent-sdk";

const DEFAULT_MODEL = "claude-sonnet-4-6";
const DEFAULT_MAX_TURNS = 10;

/**
 * CodeScope MCP server configuration for agent queries.
 * @param {string} cwd - working directory to index
 * @returns {object}
 */
export function codeScopeMcpConfig(cwd) {
  return {
    codescope: {
      command: "codescope-server",
      args: ["--mcp", "--root", cwd],
    },
  };
}

/**
 * Allowed CodeScope tools for the agent.
 * @returns {string[]}
 */
export function codeScopeAllowedTools() {
  return [
    "mcp__codescope__cs_find",
    "mcp__codescope__cs_grep",
    "mcp__codescope__cs_read_file",
    "mcp__codescope__cs_read_files",
    "mcp__codescope__cs_read_context",
    "mcp__codescope__cs_search",
    "mcp__codescope__cs_list_modules",
    "mcp__codescope__cs_get_module_files",
    "mcp__codescope__cs_find_imports",
    "mcp__codescope__cs_semantic_search",
    "mcp__codescope__cs_status",
  ];
}

/**
 * Built-in tools to block for read-only agents (prevents sub-agent spawning, file writes, etc.).
 * @returns {string[]}
 */
export function codeScopeOnlyDisallowedTools() {
  return [
    // Block sub-agent spawning
    "Task",
    // Block shell execution
    "Bash",
    // Block file writes
    "Write",
    "Edit",
    "NotebookEdit",
    // Block web access
    "WebSearch",
    "WebFetch",
    // Block interactive tools
    "AskUserQuestion",
    "ExitPlanMode",
    "TodoWrite",
    // Block built-in read tools — force use of CodeScope MCP tools instead
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
