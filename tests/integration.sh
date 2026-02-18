#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BINARY="$PROJECT_DIR/server/target/release/codescope-server"
PORT=18433
TMPDIR=$(mktemp -d)
PASSED=0
FAILED=0

cleanup() {
  kill $SERVER_PID 2>/dev/null || true
  rm -rf "$TMPDIR"
}
trap cleanup EXIT

# Ensure binary exists
if [ ! -f "$BINARY" ]; then
  echo "ERROR: Binary not found at $BINARY"
  echo "Run: cd server && cargo build --release"
  exit 1
fi

test_repo() {
  local name="$1"
  local url="$2"
  local lang="$3"

  echo ""
  echo "======================================="
  echo "  Testing: $name ($lang)"
  echo "======================================="

  # Shallow clone
  if [ ! -d "$TMPDIR/$name" ]; then
    echo "  Cloning $name..."
    git clone --depth 1 --quiet "$url" "$TMPDIR/$name"
  fi

  # Start server
  PORT=$PORT "$BINARY" --root "$TMPDIR/$name" &
  SERVER_PID=$!

  # Wait for server to be ready (max 30s)
  echo "  Waiting for server..."
  for i in $(seq 1 60); do
    if curl -s "http://localhost:$PORT/api/tree" > /dev/null 2>&1; then
      break
    fi
    if [ $i -eq 60 ]; then
      echo "  FAIL: Server did not start in 30s"
      kill $SERVER_PID 2>/dev/null || true
      FAILED=$((FAILED + 1))
      return
    fi
    sleep 0.5
  done
  echo "  Server ready."

  # Run validation
  if node "$SCRIPT_DIR/validate.mjs" "$name" "$lang" "http://localhost:$PORT"; then
    PASSED=$((PASSED + 1))
  else
    FAILED=$((FAILED + 1))
  fi

  # Stop server
  kill $SERVER_PID 2>/dev/null || true
  wait $SERVER_PID 2>/dev/null || true
  sleep 1
}

# Test repos
test_repo "ripgrep" "https://github.com/BurntSushi/ripgrep.git" "rust"
test_repo "fastify" "https://github.com/fastify/fastify.git" "javascript"
test_repo "cobra" "https://github.com/spf13/cobra.git" "go"

echo ""
echo "======================================="
echo "  Results: $PASSED passed, $FAILED failed"
echo "======================================="

if [ $FAILED -gt 0 ]; then
  exit 1
fi
