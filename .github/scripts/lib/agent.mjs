import { query } from "@anthropic-ai/claude-agent-sdk";
import { existsSync, appendFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

// Pin exact model versions to prevent behavior drift on updates
const DEFAULT_MODEL = "claude-sonnet-4-20250514";
const DEFAULT_MAX_TURNS = 10;
const DEFAULT_MAX_BUDGET_USD = 2.0;

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
 * Streams messages, logs tool usage to stderr, writes conversation
 * log to /tmp/agent-conversation-{label}.jsonl for debugging.
 *
 * Uses the SDK's built-in maxBudgetUsd for cost control and reads
 * authoritative usage from the SDKResultMessage.
 *
 * @param {{ prompt: string, systemPrompt: string, model?: string, maxTurns?: number, maxBudgetUsd?: number, codeScopeOnly?: boolean, logLabel?: string }} params
 * @returns {Promise<{ text: string, usage: { inputTokens: number, outputTokens: number, cachedTokens: number, totalCostUsd: number, turns: number, logFile: string } }>}
 */
export async function runAgent({
  prompt,
  systemPrompt,
  model = DEFAULT_MODEL,
  maxTurns = DEFAULT_MAX_TURNS,
  maxBudgetUsd = DEFAULT_MAX_BUDGET_USD,
  codeScopeOnly = false,
  logLabel = "agent",
}) {
  let lastText = "";
  let toolCallCount = 0;

  // Authoritative usage — filled from SDKResultMessage at the end
  let finalUsage = {
    inputTokens: 0,
    outputTokens: 0,
    cachedTokens: 0,
    totalCostUsd: 0,
    turns: 0,
  };

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
    maxBudgetUsd,
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
    // Assistant messages — log tool calls and capture text
    if (message.type === "assistant" && message.message?.content) {
      const toolCalls = [];

      for (const block of message.message.content) {
        if (block.type === "tool_use") {
          toolCallCount++;
          console.error(`[codescope] Using tool: ${block.name}`);
          toolCalls.push({
            name: block.name,
            args_preview: JSON.stringify(block.input || {}).substring(0, 500),
          });
        }
        if (block.type === "text") {
          lastText = block.text;
          console.error(block.text);
        }
      }

      logEntry({
        timestamp: new Date().toISOString(),
        type: "assistant",
        tool_calls: toolCalls.length > 0 ? toolCalls : undefined,
        text_preview: lastText.substring(0, 300),
      });
    }

    // Result message — authoritative usage and cost data from the SDK
    if (message.type === "result") {
      if (message.subtype === "success") {
        lastText = message.result;
        console.error(`[result] ${message.result?.substring(0, 200)}`);
      } else {
        console.error(`[result] ${message.subtype}: ${JSON.stringify(message.errors || [])}`);
      }

      // Extract authoritative usage from SDKResultMessage
      finalUsage.turns = message.num_turns || 0;
      finalUsage.totalCostUsd = message.total_cost_usd || 0;

      // modelUsage has camelCase fields: { [model]: { inputTokens, outputTokens, ... } }
      if (message.modelUsage) {
        for (const mu of Object.values(message.modelUsage)) {
          finalUsage.inputTokens += mu.inputTokens || 0;
          finalUsage.outputTokens += mu.outputTokens || 0;
          finalUsage.cachedTokens += mu.cacheReadInputTokens || 0;
        }
      }

      logEntry({
        timestamp: new Date().toISOString(),
        type: "result",
        subtype: message.subtype,
        turns: finalUsage.turns,
        cost_usd: finalUsage.totalCostUsd,
        input_tokens: finalUsage.inputTokens,
        output_tokens: finalUsage.outputTokens,
        cached_tokens: finalUsage.cachedTokens,
        tool_calls_total: toolCallCount,
        errors: message.errors,
      });
    }
  }

  const usageSummary = {
    ...finalUsage,
    logFile,
  };

  console.error(
    `[agent] Done: ${finalUsage.turns} turns, ` +
    `${finalUsage.inputTokens.toLocaleString()} in / ${finalUsage.outputTokens.toLocaleString()} out ` +
    `(${finalUsage.cachedTokens.toLocaleString()} cached), ` +
    `~$${finalUsage.totalCostUsd.toFixed(2)}`
  );

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
  // Match all JSON objects in the text (handles nested objects)
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
