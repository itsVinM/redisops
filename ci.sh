#!/bin/bash
# RedisOps CI - local continuous integration script
# Runs all tests, checks compilation, validates embedded targets
# Run: bash ci.sh

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$PROJECT_DIR"

PASS=0
FAIL=0

step() {
    echo -e "\n${CYAN}[$1]${NC} $2"
}

ok() {
    echo -e "  ${GREEN}✓${NC} $1"
    PASS=$((PASS + 1))
}

fail() {
    echo -e "  ${RED}✗${NC} $1"
    FAIL=$((FAIL + 1))
}

# ── 1. Format check ──
step "1/5" "Code formatting"
if cargo fmt --check --manifest-path redisdb/Cargo.toml 2>/dev/null; then
    ok "Format check passed"
else
    fail "Run 'cargo fmt' to fix"
fi

# ── 2. Clippy lints ──
step "2/5" "Clippy lints"
if cargo clippy --manifest-path redisdb/Cargo.toml 2>&1 | grep -q "error\["; then
    fail "Clippy errors found (run 'cargo clippy' to see)"
else
    ok "No clippy errors"
fi

# ── 3. Debug build + tests ──
step "3/5" "Debug build + unit tests"
TEST_OUTPUT=$(cargo test --manifest-path redisdb/Cargo.toml 2>&1)
if echo "$TEST_OUTPUT" | grep -q "test result: ok"; then
    total=$(echo "$TEST_OUTPUT" | grep "test result: ok" | awk '{s+=$4} END {print s}')
    ok "All tests passed ($total tests)"
else
    fail "Tests failed"
fi

# ── 4. Release build ──
step "4/5" "Release build"
if cargo build --release --manifest-path redisdb/Cargo.toml 2>/dev/null; then
    size=$(du -h target/release/redisops 2>/dev/null | cut -f1)
    ok "Release binary built ($size)"
else
    fail "Release build failed"
fi

# ── 5. Check lib compiles (all modules) ──
step "5/5" "Library compilation check"
if cargo check --lib --manifest-path redisdb/Cargo.toml 2>/dev/null; then
    ok "All library modules compile"
else
    fail "Library compilation failed"
fi

# ── Summary ──
echo ""
echo "════════════════════════════════════════════════"
if [[ $FAIL -eq 0 ]]; then
    echo -e "  ${GREEN}CI PASSED${NC}: $PASS checks OK"
    exit 0
else
    echo -e "  ${RED}CI FAILED${NC}: $FAIL failures, $PASS passed"
    exit 1
fi
