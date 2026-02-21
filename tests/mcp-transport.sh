#!/bin/bash
# MCP HTTP Transport Tests — validates OAuth discovery, origin validation,
# session lifecycle, and protocol compliance per MCP 2025-11-25.
#
# Addresses every finding from authprobe scan (GitHub issue #1):
#   - PRM endpoint reachable
#   - Init ordering enforced
#   - Protocol version negotiated
#   - Origin validated
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BINARY="$PROJECT_DIR/server/target/release/codescope"
PORT=18434
BASE="http://localhost:$PORT"
PASSED=0
FAILED=0
SERVER_PID=""

cleanup() {
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true
}
trap cleanup EXIT

if [ ! -f "$BINARY" ]; then
  echo "ERROR: Binary not found at $BINARY"
  echo "Run: cd server && cargo build --release"
  exit 1
fi

# ── Helpers ──

assert_status() {
  local desc="$1" expected="$2" actual="$3"
  if [ "$actual" = "$expected" ]; then
    echo "  PASS: $desc (HTTP $actual)"
    PASSED=$((PASSED + 1))
  else
    echo "  FAIL: $desc — expected HTTP $expected, got $actual"
    FAILED=$((FAILED + 1))
  fi
}

assert_contains() {
  local desc="$1" haystack="$2" needle="$3"
  if echo "$haystack" | grep -q "$needle"; then
    echo "  PASS: $desc"
    PASSED=$((PASSED + 1))
  else
    echo "  FAIL: $desc — response missing '$needle'"
    FAILED=$((FAILED + 1))
  fi
}

# JSON-RPC helpers
INIT_REQ='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"ci-test","version":"1.0"}}}'
TOOLS_LIST='{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
NOTIFICATION='{"jsonrpc":"2.0","method":"notifications/initialized"}'

# ── Start server ──

echo "Starting server on port $PORT..."
PORT=$PORT "$BINARY" --root "$PROJECT_DIR" --no-semantic &
SERVER_PID=$!

for i in $(seq 1 40); do
  if curl -sf "$BASE/health" > /dev/null 2>&1; then break; fi
  if [ "$i" -eq 40 ]; then echo "FAIL: Server did not start"; exit 1; fi
  sleep 0.25
done
echo "Server ready (PID $SERVER_PID)."
echo ""

# ══════════════════════════════════════════════
#  1. PRM Endpoint (RFC 9728)
# ══════════════════════════════════════════════
echo "--- PRM Endpoint ---"

PRM_RESP=$(curl -sf "$BASE/.well-known/oauth-protected-resource/mcp")
PRM_STATUS=$(curl -so /dev/null -w "%{http_code}" "$BASE/.well-known/oauth-protected-resource/mcp")

assert_status "PRM returns 200" "200" "$PRM_STATUS"
assert_contains "PRM has 'resource' field" "$PRM_RESP" '"resource"'
assert_contains "PRM has 'authorization_servers' field" "$PRM_RESP" '"authorization_servers"'
echo ""

# ══════════════════════════════════════════════
#  2. GET /mcp → 405
# ══════════════════════════════════════════════
echo "--- GET /mcp (should be 405) ---"

GET_STATUS=$(curl -so /dev/null -w "%{http_code}" "$BASE/mcp")
assert_status "GET /mcp rejected" "405" "$GET_STATUS"
echo ""

# ══════════════════════════════════════════════
#  3. Malformed JSON → 400
# ══════════════════════════════════════════════
echo "--- Malformed JSON ---"

BAD_RESP=$(curl -s -w "\n%{http_code}" -X POST "$BASE/mcp" \
  -H "Content-Type: application/json" \
  -d "not json at all")
BAD_STATUS=$(echo "$BAD_RESP" | tail -1)
BAD_BODY=$(echo "$BAD_RESP" | sed '$d')

assert_status "Malformed JSON returns 400" "400" "$BAD_STATUS"
assert_contains "Parse error in response" "$BAD_BODY" "Parse error"
echo ""

# ══════════════════════════════════════════════
#  4. Init Ordering — tools/list before initialize
# ══════════════════════════════════════════════
echo "--- Init Ordering ---"

PRE_INIT_STATUS=$(curl -so /dev/null -w "%{http_code}" -X POST "$BASE/mcp" \
  -H "Content-Type: application/json" \
  -d "$TOOLS_LIST")

assert_status "tools/list without init rejected" "400" "$PRE_INIT_STATUS"
echo ""

# ══════════════════════════════════════════════
#  5. Initialize → Session + Version Negotiation
# ══════════════════════════════════════════════
echo "--- Initialize ---"

INIT_FULL=$(curl -s -D - -X POST "$BASE/mcp" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  -d "$INIT_REQ")

# Extract session ID from response headers
SESSION_ID=$(echo "$INIT_FULL" | grep -i "mcp-session-id:" | tr -d '\r' | awk '{print $2}')
INIT_BODY=$(echo "$INIT_FULL" | sed '1,/^\r*$/d')

if [ -n "$SESSION_ID" ]; then
  echo "  PASS: Got session ID: ${SESSION_ID:0:8}..."
  PASSED=$((PASSED + 1))
else
  echo "  FAIL: No Mcp-Session-Id header in response"
  FAILED=$((FAILED + 1))
fi

assert_contains "Initialize has protocolVersion" "$INIT_BODY" "protocolVersion"
assert_contains "Initialize has serverInfo" "$INIT_BODY" "serverInfo"
echo ""

# ══════════════════════════════════════════════
#  6. Send notification (should be 202)
# ══════════════════════════════════════════════
echo "--- Notification ---"

NOTIF_STATUS=$(curl -so /dev/null -w "%{http_code}" -X POST "$BASE/mcp" \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: $SESSION_ID" \
  -d "$NOTIFICATION")

assert_status "Notification returns 202" "202" "$NOTIF_STATUS"
echo ""

# ══════════════════════════════════════════════
#  7. tools/list with valid session
# ══════════════════════════════════════════════
echo "--- tools/list (valid session) ---"

TOOLS_RESP=$(curl -s -w "\n%{http_code}" -X POST "$BASE/mcp" \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: $SESSION_ID" \
  -H "Mcp-Protocol-Version: 2025-11-25" \
  -d "$TOOLS_LIST")
TOOLS_STATUS=$(echo "$TOOLS_RESP" | tail -1)
TOOLS_BODY=$(echo "$TOOLS_RESP" | sed '$d')

assert_status "tools/list with session returns 200" "200" "$TOOLS_STATUS"
assert_contains "Response has tools" "$TOOLS_BODY" '"tools"'
assert_contains "cs_search in tool list" "$TOOLS_BODY" "cs_search"
echo ""

# ══════════════════════════════════════════════
#  8. Invalid session ID → 400
# ══════════════════════════════════════════════
echo "--- Invalid Session ---"

BAD_SID_STATUS=$(curl -so /dev/null -w "%{http_code}" -X POST "$BASE/mcp" \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: bogus-session-id-12345" \
  -d "$TOOLS_LIST")

assert_status "Invalid session ID rejected" "400" "$BAD_SID_STATUS"
echo ""

# ══════════════════════════════════════════════
#  9. Protocol version mismatch → 400
# ══════════════════════════════════════════════
echo "--- Protocol Version Mismatch ---"

PV_STATUS=$(curl -so /dev/null -w "%{http_code}" -X POST "$BASE/mcp" \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: $SESSION_ID" \
  -H "Mcp-Protocol-Version: 9999-99-99" \
  -d "$TOOLS_LIST")

assert_status "Wrong protocol version rejected" "400" "$PV_STATUS"
echo ""

# ══════════════════════════════════════════════
#  10. Origin validation
# ══════════════════════════════════════════════
echo "--- Origin Validation ---"

# Good origin
GOOD_ORIGIN_STATUS=$(curl -so /dev/null -w "%{http_code}" -X POST "$BASE/mcp" \
  -H "Content-Type: application/json" \
  -H "Origin: http://localhost:$PORT" \
  -d "$INIT_REQ")

assert_status "Allowed origin accepted" "200" "$GOOD_ORIGIN_STATUS"

# Bad origin
BAD_ORIGIN_STATUS=$(curl -so /dev/null -w "%{http_code}" -X POST "$BASE/mcp" \
  -H "Content-Type: application/json" \
  -H "Origin: http://evil.example.com" \
  -d "$INIT_REQ")

assert_status "Forbidden origin rejected" "403" "$BAD_ORIGIN_STATUS"

# No origin (non-browser client)
NO_ORIGIN_STATUS=$(curl -so /dev/null -w "%{http_code}" -X POST "$BASE/mcp" \
  -H "Content-Type: application/json" \
  -d "$INIT_REQ")

assert_status "Missing origin allowed (non-browser)" "200" "$NO_ORIGIN_STATUS"
echo ""

# ══════════════════════════════════════════════
#  11. DELETE /mcp — Session termination
# ══════════════════════════════════════════════
echo "--- Session Termination ---"

DEL_STATUS=$(curl -so /dev/null -w "%{http_code}" -X DELETE "$BASE/mcp" \
  -H "Mcp-Session-Id: $SESSION_ID")

assert_status "DELETE session returns 200" "200" "$DEL_STATUS"

# Verify session is gone
POST_DEL_STATUS=$(curl -so /dev/null -w "%{http_code}" -X POST "$BASE/mcp" \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: $SESSION_ID" \
  -d "$TOOLS_LIST")

assert_status "Deleted session rejected" "400" "$POST_DEL_STATUS"
echo ""

# ══════════════════════════════════════════════
#  Results
# ══════════════════════════════════════════════
echo "======================================="
echo "  MCP Transport: $PASSED passed, $FAILED failed"
echo "======================================="

if [ $FAILED -gt 0 ]; then
  exit 1
fi
