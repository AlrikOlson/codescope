#!/usr/bin/env node
// AI-powered semver bump using Claude Agent SDK + CodeScope MCP server.
// Runs a Claude Code session with full codebase search capabilities
// to analyze changes and determine the correct version bump.

import { query } from "@anthropic-ai/claude-agent-sdk";
import { execSync } from "child_process";

const lastTag = process.argv[2] || "v0.0.0";

function git(cmd) {
  try {
    return execSync(`git ${cmd}`, { encoding: "utf-8", timeout: 30000 }).trim();
  } catch {
    return "";
  }
}

// Gather git context for the prompt
const refRange = lastTag !== "v0.0.0" ? `${lastTag}..HEAD` : "HEAD~20..HEAD";
const commits = git(`log ${refRange} --pretty=format:"%h %s"`) || git('log --pretty=format:"%h %s" -20');
const diffStat = git(`diff --stat ${refRange}`).slice(-2000) || "(unavailable)";

const prompt = `You are analyzing changes to CodeScope (a Rust MCP server + TypeScript web UI for codebase search and navigation) to determine the correct semantic version bump.

You have access to the CodeScope MCP server tools. Use them to explore the codebase and understand the impact of the changes.

LAST TAG: ${lastTag}
COMMITS SINCE LAST TAG:
${commits}

FILES CHANGED:
${diffStat}

## Your Task

1. Use the CodeScope tools (cs_find, cs_grep, cs_read_file, cs_read_context) to read the changed files and understand what was modified.
2. Check if any public APIs, MCP tool interfaces, CLI flags, or config formats changed in breaking ways.
3. Check if new features, tools, or capabilities were added.
4. Determine the correct semver bump:
   - MAJOR: breaking changes to MCP protocol, CLI interface, API endpoints, or config format that would break existing users/integrations
   - MINOR: new features, new MCP tools, new CLI flags, new API endpoints, meaningful new capabilities
   - PATCH: bug fixes, performance improvements, refactoring, documentation, CI/CD changes, dependency updates, workflow changes

When in doubt between minor and patch, prefer patch. Only use major for genuine breaking changes.

After your analysis, your FINAL line of output must be EXACTLY one JSON object:
{"bump":"patch","reason":"one sentence explanation"}

No markdown fencing, no extra text after the JSON. The JSON must be the last line.`;

async function main() {
  let lastText = "";

  try {
    for await (const message of query({
      prompt,
      options: {
        model: "claude-sonnet-4-6",
        maxTurns: 10,
        systemPrompt: "You are a precise semver version bump analyzer. Use the available CodeScope tools to deeply understand the changes before making your decision. Be thorough but concise.",
        mcpServers: {
          codescope: {
            command: "codescope-server",
            args: ["--mcp", "--root", process.cwd()],
          },
        },
        allowedTools: [
          "mcp__codescope__cs_find",
          "mcp__codescope__cs_grep",
          "mcp__codescope__cs_read_file",
          "mcp__codescope__cs_read_files",
          "mcp__codescope__cs_read_context",
          "mcp__codescope__cs_search",
          "mcp__codescope__cs_list_modules",
          "mcp__codescope__cs_get_module_files",
          "mcp__codescope__cs_find_imports",
          "mcp__codescope__cs_status",
        ],
        permissionMode: "bypassPermissions",
        allowDangerouslySkipPermissions: true,
        cwd: process.cwd(),
      },
    })) {
      // SDKAssistantMessage: content is at message.message.content
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

      // SDKResultMessage: final result
      if ("result" in message && message.subtype === "success") {
        lastText = message.result;
        console.error(`[result] ${message.result}`);
      } else if ("result" in message) {
        console.error(`[error] ${JSON.stringify(message)}`);
      }
    }
  } catch (err) {
    console.error(`Agent SDK error: ${err.message}`);
    output("patch", "Agent SDK error, defaulting to patch");
    return;
  }

  // Extract the JSON from the last text output
  const jsonMatch = lastText.match(/\{[^{}]*"bump"\s*:\s*"(major|minor|patch)"[^{}]*\}/);
  if (jsonMatch) {
    try {
      const result = JSON.parse(jsonMatch[0]);
      const bump = result.bump || "patch";
      const reason = result.reason || "no reason provided";
      output(bump, reason);
      return;
    } catch {
      // fall through
    }
  }

  console.error("Could not parse bump from Claude output — defaulting to patch");
  output("patch", "Could not parse AI response");
}

function output(bump, reason) {
  console.error(`AI Bump: ${bump} (${reason})`);
  // Write to stdout for the shell to capture
  process.stdout.write(bump);
}

main().catch((err) => {
  console.error(`Fatal: ${err.message}`);
  process.stdout.write("patch");
  process.exit(0); // Don't fail the workflow, just default to patch
});
