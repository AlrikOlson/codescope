import { query } from "@anthropic-ai/claude-agent-sdk";
import { existsSync, appendFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

// Pin exact model versions to prevent behavior drift on updates
const DEFAULT_MODEL = "claude-sonnet-4-20250514";
const DEFAULT_MAX_TURNS = 10;
const DEFAULT_MAX_COST_USD = 2.0;

// Approximate per-token costs (USD) — Sonnet 4
const COST_PER_INPUT_TOKEN = 3.0 / 1_000_000;
const COST_PER_OUTPUT_TOKEN = 15.0 / 1_000_000;
const COST_PER_CACHED_TOKEN = 0.3 / 1_000_000;

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
    // Block planning/todo tools (wastes agent turns)
    "TodoRead",
    "TodoWrite",
    // Block built-in read tools — force use of CodeScope MCP tools
    "Read",
    "Glob",
    "Grep",
  ];
}

/**
 * Estimate cost from token usage.
 * @param {{ input_tokens?: number, output_tokens?: number, cache_read_input_tokens?: number }} usage
 * @returns {number} estimated cost in USD
 */
function estimateCost(usage) {
  if (!usage) return 0;
  const input = (usage.input_tokens || 0) - (usage.cache_read_input_tokens || 0);
  const cached = usage.cache_read_input_tokens || 0;
  const output = usage.output_tokens || 0;
  return input * COST_PER_INPUT_TOKEN + cached * COST_PER_CACHED_TOKEN + output * COST_PER_OUTPUT_TOKEN;
}

/**
 * Write a step summary to GITHUB_STEP_SUMMARY (visible in Actions UI).
 * No-op outside CI.
 * @param {string} markdown
 */
export function writeStepSummary(markdown) {
  const summaryFile = process.env.GITHUB_STEP_SUMMARY;
  if (summaryFile) {
    appendFileSync(summaryFile, markdown + "\n");
  }
}

/**
 * Run a Claude Agent SDK query with CodeScope MCP.
 * Streams messages, logs tool usage to stderr, tracks cost, writes conversation
 * log to /tmp/agent-conversation-{label}.jsonl for debugging.
 *
 * @param {{ prompt: string, systemPrompt: string, model?: string, maxTurns?: number, maxCostUsd?: number, codeScopeOnly?: boolean, logLabel?: string }} params
 * @returns {Promise<{ text: string, usage: { inputTokens: number, outputTokens: number, cachedTokens: number, totalCostUsd: number, turns: number } }>}
 */
export async function runAgent({
  prompt,
  systemPrompt,
  model = DEFAULT_MODEL,
  maxTurns = DEFAULT_MAX_TURNS,
  maxCostUsd = DEFAULT_MAX_COST_USD,
  codeScopeOnly = false,
  logLabel = "agent",
}) {
  let lastText = "";
  let turnCount = 0;
  let totalInputTokens = 0;
  let totalOutputTokens = 0;
  let totalCachedTokens = 0;
  let totalCostUsd = 0;

  const logFile = `/tmp/agent-conversation-${logLabel}.jsonl`;
  // Clear previous log
  writeFileSync(logFile, "");

  function logEntry(entry) {
    try {
      appendFileSync(logFile, JSON.stringify(entry) + "\n");
    } catch {
      // Best-effort logging
    }
  }

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
      turnCount++;
      const toolCalls = [];

      for (const block of message.message.content) {
        if (block.type === "tool_use") {
          console.error(`[codescope] Using tool: ${block.name}`);
          toolCalls.push({ name: block.name, args_preview: JSON.stringify(block.input || {}).substring(0, 500) });
        }
        if (block.type === "text") {
          lastText = block.text;
          console.error(block.text);
        }
      }

      // Track token usage if available
      const usage = message.message?.usage;
      if (usage) {
        totalInputTokens += usage.input_tokens || 0;
        totalOutputTokens += usage.output_tokens || 0;
        totalCachedTokens += usage.cache_read_input_tokens || 0;
        totalCostUsd += estimateCost(usage);
      }

      // Log conversation turn
      logEntry({
        timestamp: new Date().toISOString(),
        turn: turnCount,
        tool_calls: toolCalls.length > 0 ? toolCalls : undefined,
        text_preview: lastText.substring(0, 300),
        usage,
        cumulative_cost_usd: Number(totalCostUsd.toFixed(4)),
      });

      // Cost circuit breaker
      if (totalCostUsd > maxCostUsd) {
        console.error(`[agent] Cost limit exceeded: $${totalCostUsd.toFixed(2)} > $${maxCostUsd.toFixed(2)} — aborting`);
        logEntry({ type: "abort", reason: "cost_limit", cost_usd: totalCostUsd });
        break;
      }
    }

    if ("result" in message && message.subtype === "success") {
      lastText = message.result;
      console.error(`[result] ${message.result}`);
    } else if ("result" in message) {
      console.error(`[error] ${JSON.stringify(message)}`);
    }
  }

  const usageSummary = {
    inputTokens: totalInputTokens,
    outputTokens: totalOutputTokens,
    cachedTokens: totalCachedTokens,
    totalCostUsd: Number(totalCostUsd.toFixed(4)),
    turns: turnCount,
    logFile,
  };

  console.error(`[agent] Done: ${turnCount} turns, ${totalInputTokens} in / ${totalOutputTokens} out, ~$${totalCostUsd.toFixed(2)}`);

  return { text: lastText, usage: usageSummary };
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
