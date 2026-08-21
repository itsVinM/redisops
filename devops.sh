#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────────────────────
#  devops.sh — orchestrate Redis server + C++20 sandbox
# ──────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REDIS_DIR="$SCRIPT_DIR/redisdb"
SANDBOX_DIR="$SCRIPT_DIR/sandbox"
BUILD_DIR="$SANDBOX_DIR/build"
LOG_DIR="$SCRIPT_DIR/.devops-logs"
PID_DIR="$SCRIPT_DIR/.devops-pids"
REDIS_HOST="${REDIS_HOST:-127.0.0.1}"
REDIS_PORT="${REDIS_PORT:-1234}"
DOCKER_IMAGE="${DOCKER_IMAGE:-devops-sandbox}"
USE_DOCKER="${USE_DOCKER:-false}"

# ── Colors ──

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

log()   { echo -e "${CYAN}[devops]${NC} $*"; }
ok()    { echo -e "${GREEN}[  ok ]${NC} $*"; }
warn()  { echo -e "${YELLOW}[warn]${NC} $*"; }
err()   { echo -e "${RED}[error]${NC} $*" >&2; }

# ──────────────────────────────────────────────────────────────
#  BUILD
# ──────────────────────────────────────────────────────────────

build_rust() {
    log "Building Rust server..."
    cd "$REDIS_DIR"
    cargo build --release 2>&1 | tail -1
    ok "Rust server built"
}

build_cpp() {
    log "Building C++20 sandbox..."
    mkdir -p "$BUILD_DIR"
    cd "$BUILD_DIR"
    cmake -DCMAKE_BUILD_TYPE=Release .. 2>&1 | tail -3
    cmake --build . -j"$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)" 2>&1 | tail -1
    ok "C++20 sandbox built"
}

build_docker() {
    log "Building Docker image..."
    cd "$REDIS_DIR"
    docker build -t "$DOCKER_IMAGE" -f sandbox/Dockerfile . 2>&1 | tail -3
    ok "Docker image built: $DOCKER_IMAGE"
}

cmd_build() {
    build_rust
    build_cpp
    if [ "$USE_DOCKER" = "true" ] || [ "${1:-}" = "--docker" ]; then
        build_docker
    fi
    ok "All builds complete"
}

# ──────────────────────────────────────────────────────────────
#  START
# ──────────────────────────────────────────────────────────────

start_redis() {
    if pgrep -f "redisops" > /dev/null 2>&1; then
        warn "Redis server already running"
        return 0
    fi

    log "Starting Redis server on ${REDIS_HOST}:${REDIS_PORT}..."
    mkdir -p "$LOG_DIR" "$PID_DIR"

    RUST_LOG=info "$REDIS_DIR/target/release/redisops" \
        > "$LOG_DIR/redis.log" 2>&1 &
    echo $! > "$PID_DIR/redis.pid"

    # Wait for server to be ready
    for i in $(seq 1 20); do
        if bash -c "echo ping > /dev/tcp/${REDIS_HOST}/${REDIS_PORT}" 2>/dev/null; then
            ok "Redis server ready (pid=$(cat "$PID_DIR/redis.pid"))"
            return 0
        fi
        sleep 0.2
    done
    err "Redis server failed to start"
    cat "$LOG_DIR/redis.log" | tail -10
    return 1
}

start_sandbox() {
    if [ "$USE_DOCKER" = "true" ]; then
        start_sandbox_docker
    else
        start_sandbox_native
    fi
}

start_sandbox_native() {
    if pgrep -f "devops-sandbox" > /dev/null 2>&1; then
        warn "Sandbox already running"
        return 0
    fi

    if [ ! -f "$BUILD_DIR/sandbox" ]; then
        err "Sandbox binary not found. Run: ./devops.sh build"
        return 1
    fi

    log "Starting C++20 sandbox..."
    "$BUILD_DIR/sandbox" \
        --host "$REDIS_HOST" \
        --port "$REDIS_PORT" \
        --id "sandbox-1" \
        --type local \
        > "$LOG_DIR/sandbox.log" 2>&1 &
    echo $! > "$PID_DIR/sandbox.pid"
    ok "Sandbox started (pid=$(cat "$PID_DIR/sandbox.pid"))"
}

start_sandbox_docker() {
    if docker ps --format '{{.Names}}' | grep -q "devops-sandbox"; then
        warn "Docker sandbox already running"
        return 0
    fi

    log "Starting Docker sandbox..."
    docker run -d \
        --name devops-sandbox \
        --network host \
        -v "$REDIS_DIR:/app" \
        -w /app \
        "$DOCKER_IMAGE" \
        /app/sandbox/build/sandbox \
            --host "$REDIS_HOST" \
            --port "$REDIS_PORT" \
            --id "docker-sandbox-1" \
            --type docker \
        > "$LOG_DIR/sandbox-docker.log" 2>&1
    ok "Docker sandbox started"
}

cmd_start() {
    start_redis
    start_sandbox
    echo ""
    log "System ready. Submit jobs with: ./devops.sh submit <name> <command>"
}

# ──────────────────────────────────────────────────────────────
#  STOP
# ──────────────────────────────────────────────────────────────

stop_process() {
    local name="$1"
    local pidfile="$PID_DIR/$name.pid"
    if [ -f "$pidfile" ]; then
        local pid
        pid=$(cat "$pidfile")
        if kill -0 "$pid" 2>/dev/null; then
            log "Stopping $name (pid=$pid)..."
            kill "$pid"
            sleep 0.5
            if kill -0 "$pid" 2>/dev/null; then
                kill -9 "$pid" 2>/dev/null || true
            fi
            ok "$name stopped"
        fi
        rm -f "$pidfile"
    fi
}

cmd_stop() {
    if [ "$USE_DOCKER" = "true" ]; then
        docker stop devops-sandbox 2>/dev/null && ok "Docker sandbox stopped" || true
        docker rm devops-sandbox 2>/dev/null || true
    fi
    stop_process sandbox
    stop_process redis
    ok "All stopped"
}

cmd_restart() {
    cmd_stop
    sleep 1
    cmd_start
}

# ──────────────────────────────────────────────────────────────
#  STATUS
# ──────────────────────────────────────────────────────────────

cmd_status() {
    echo ""
    echo -e "${CYAN}═══════════════════════════════════════════${NC}"
    echo -e "${CYAN}  devops-platform status${NC}"
    echo -e "${CYAN}═══════════════════════════════════════════${NC}"
    echo ""

    # Redis server
    if [ -f "$PID_DIR/redis.pid" ] && kill -0 "$(cat "$PID_DIR/redis.pid")" 2>/dev/null; then
        echo -e "  Redis server:  ${GREEN}running${NC} (pid=$(cat "$PID_DIR/redis.pid"))"
    else
        echo -e "  Redis server:  ${RED}stopped${NC}"
    fi

    # Sandbox
    if [ "$USE_DOCKER" = "true" ]; then
        if docker ps --format '{{.Names}}' | grep -q "devops-sandbox"; then
            echo -e "  Sandbox:       ${GREEN}running${NC} (docker)"
        else
            echo -e "  Sandbox:       ${RED}stopped${NC}"
        fi
    elif [ -f "$PID_DIR/sandbox.pid" ] && kill -0 "$(cat "$PID_DIR/sandbox.pid")" 2>/dev/null; then
        echo -e "  Sandbox:       ${GREEN}running${NC} (pid=$(cat "$PID_DIR/sandbox.pid"))"
    else
        echo -e "  Sandbox:       ${RED}stopped${NC}"
    fi

    echo ""
    echo -e "  Redis:   ${REDIS_HOST}:${REDIS_PORT}"
    echo -e "  Logs:    $LOG_DIR/"
    echo ""
}

# ──────────────────────────────────────────────────────────────
#  JOB MANAGEMENT
# ──────────────────────────────────────────────────────────────

# Helper: send a raw command to the redis-rs binary protocol
# Since the server uses a custom binary protocol (not TCP RESP),
# we use the redis-tui binary or a Python helper.
redis_cmd() {
    # Use Python to send via TCP (the server speaks binary protocol on port 1234)
    python3 -c "
import socket, struct, sys

def send_cmd(host, port, args):
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.connect((host, port))
    s.settimeout(5)
    # Encode: [u32 msg_len][u32 n_args][u32 arg_len][arg_bytes]...
    payload = struct.pack('<I', len(args))
    for arg in args:
        b = arg.encode()
        payload += struct.pack('<I', len(b)) + b
    msg = struct.pack('<I', len(payload)) + payload
    s.sendall(msg)
    # Read response
    raw_len = s.recv(4)
    if len(raw_len) < 4:
        return None
    resp_len = struct.unpack('<I', raw_len)[0]
    data = s.recv(resp_len)
    s.close()
    return data

def parse_resp(data):
    if not data:
        return 'nil'
    tag = data[0]
    if tag == 0: return 'nil'
    if tag == 1:  # error
        code = struct.unpack('<i', data[1:5])[0]
        msg_len = struct.unpack('<I', data[5:9])[0]
        return f'ERR({code}): {data[9:9+msg_len].decode()}'
    if tag == 2:  # string
        slen = struct.unpack('<I', data[1:5])[0]
        return data[5:5+slen].decode('utf-8', errors='replace')
    if tag == 3:  # int
        return str(struct.unpack('<q', data[1:9])[0])
    if tag == 4:  # double
        return str(struct.unpack('<d', data[1:9])[0])
    if tag == 5:  # array
        count = struct.unpack('<I', data[1:5])[0]
        pos = 5
        items = []
        for _ in range(count):
            elem_tag = data[pos]
            if elem_tag == 0:
                items.append('nil')
                pos += 1
            elif elem_tag == 2:
                slen = struct.unpack('<I', data[pos+1:pos+5])[0]
                items.append(data[pos+5:pos+5+slen].decode('utf-8', errors='replace'))
                pos += 5 + slen
            elif elem_tag == 3:
                items.append(str(struct.unpack('<q', data[pos+1:pos+9])[0]))
                pos += 9
            elif elem_tag == 4:
                items.append(str(struct.unpack('<d', data[pos+1:pos+9])[0]))
                pos += 9
            else:
                items.append(f'unknown({elem_tag})')
                pos += 1
        return items
    return f'unknown({tag})'

data = send_cmd('${REDIS_HOST}', ${REDIS_PORT}, sys.argv[1:])
result = parse_resp(data)
if isinstance(result, list):
    for item in result:
        print(item)
else:
    print(result)
" "$@"
}

cmd_submit() {
    local name="${1:?usage: devops.sh submit <name> <command> [target]}"
    local cmd="${2:?usage: devops.sh submit <name> <command> [target]}"
    local target="${3:-local}"
    local job_id="job-$(date +%s)-$$"

    log "Submitting job: $name (target=$target)"
    redis_cmd "job" "submit" "$job_id" "$name" "$target" "$cmd"
    ok "Job submitted: $job_id"
    echo "$job_id"
}

cmd_next() {
    log "Fetching next job..."
    redis_cmd "job" "next"
}

cmd_result() {
    local job_id="${1:?usage: devops.sh result <job_id> <exit_code> <duration_ms>}"
    local exit_code="${2:?missing exit_code}"
    local duration="${3:?missing duration_ms}"
    redis_cmd "job" "result" "$job_id" "$exit_code" "$duration"
}

cmd_list() {
    local filter="${1:-}"
    if [ -n "$filter" ]; then
        redis_cmd "job" "list" "$filter"
    else
        redis_cmd "job" "list"
    fi
}

cmd_job_status() {
    local job_id="${1:?usage: devops.sh job-status <job_id>}"
    redis_cmd "job" "status" "$job_id"
}

cmd_job_log() {
    local job_id="${1:?usage: devops.sh job-log <job_id>}"
    redis_cmd "job" "log" "$job_id"
}

cmd_metric_summary() {
    redis_cmd "metric" "summary"
}

cmd_sandbox_status() {
    redis_cmd "sandbox" "status"
}

# ──────────────────────────────────────────────────────────────
#  TEST
# ──────────────────────────────────────────────────────────────

cmd_test() {
    log "Running all tests..."
    echo ""

    # Rust tests
    log "── Rust tests ──"
    cd "$REDIS_DIR"
    cargo test 2>&1
    echo ""

    # C++ tests (if test binary exists)
    if [ -f "$BUILD_DIR/sandbox_test" ]; then
        log "── C++ tests ──"
        "$BUILD_DIR/sandbox_test" 2>&1
        echo ""
    fi

    ok "All tests passed"
}

# ──────────────────────────────────────────────────────────────
#  LOGS
# ──────────────────────────────────────────────────────────────

cmd_logs() {
    local target="${1:-all}"
    case "$target" in
        redis|server)
            tail -f "$LOG_DIR/redis.log" 2>/dev/null || warn "No Redis logs"
            ;;
        sandbox)
            tail -f "$LOG_DIR/sandbox.log" 2>/dev/null || warn "No sandbox logs"
            ;;
        all|*)
            if [ -f "$LOG_DIR/redis.log" ] && [ -f "$LOG_DIR/sandbox.log" ]; then
                tail -f "$LOG_DIR/redis.log" "$LOG_DIR/sandbox.log"
            elif [ -f "$LOG_DIR/redis.log" ]; then
                tail -f "$LOG_DIR/redis.log"
            elif [ -f "$LOG_DIR/sandbox.log" ]; then
                tail -f "$LOG_DIR/sandbox.log"
            else
                warn "No log files found"
            fi
            ;;
    esac
}

# ──────────────────────────────────────────────────────────────
#  DEMO
# ──────────────────────────────────────────────────────────────

cmd_demo() {
    log "Running demo pipeline..."
    echo ""

    cmd_status
    echo ""

    # Submit a few test jobs
    log "Submitting test jobs..."
    local j1 j2 j3
    j1=$(cmd_submit "echo-hello" "echo hello world")
    j2=$(cmd_submit "echo-date" "date")
    j3=$(cmd_submit "test-pass" "true")
    echo ""

    # Show queue
    cmd_list
    echo ""

    # Process jobs
    log "Processing jobs..."
    for i in 1 2 3; do
        local job
        job=$(redis_cmd "job" "next")
        if [ "$job" = "nil" ] || [ -z "$job" ]; then
            warn "No more jobs in queue"
            break
        fi
        log "Got job: $job"
        # Simulate execution
        sleep 0.2
        redis_cmd "job" "result" "$(echo "$job" | head -1)" "0" "200"
        ok "Job completed"
    done
    echo ""

    # Show summary
    cmd_metric_summary
    echo ""
    ok "Demo complete"
}

# ──────────────────────────────────────────────────────────────
#  CLEAN
# ──────────────────────────────────────────────────────────────

cmd_clean() {
    cmd_stop 2>/dev/null || true
    rm -rf "$LOG_DIR" "$PID_DIR" "$BUILD_DIR"
    cd "$REDIS_DIR"
    cargo clean 2>/dev/null || true
    ok "Cleaned"
}

# ──────────────────────────────────────────────────────────────
#  HELP
# ──────────────────────────────────────────────────────────────

cmd_help() {
    echo ""
    echo -e "${CYAN}devops.sh${NC} — validation & orchestration platform"
    echo ""
    echo "Usage: ./devops.sh <command> [args]"
    echo ""
    echo "System:"
    echo "  build [--docker]   Build Rust server + C++20 sandbox"
    echo "  start              Start Redis server + sandbox"
    echo "  stop               Stop all services"
    echo "  restart            Restart all services"
    echo "  status             Show system status"
    echo "  clean              Stop and remove all build artifacts"
    echo ""
    echo "Jobs:"
    echo "  submit <name> <cmd> [target]   Submit a validation job"
    echo "  next                           Fetch next job from queue"
    echo "  result <id> <exit> <ms>        Record job result"
    echo "  list [status]                  List jobs (optionally filter by status)"
    echo "  job-status <id>                Check job status"
    echo "  job-log <id>                   View job log"
    echo ""
    echo "Metrics:"
    echo "  metric-summary                 Show aggregated metrics"
    echo "  sandbox-status                 List active sandboxes"
    echo ""
    echo "Other:"
    echo "  test                           Run all tests (Rust + C++)"
    echo "  logs [redis|sandbox|all]       Tail log files"
    echo "  demo                           Run demo pipeline"
    echo "  help                           Show this help"
    echo ""
    echo "Docker mode: USE_DOCKER=true ./devops.sh start"
    echo ""
}

# ──────────────────────────────────────────────────────────────
#  MAIN
# ──────────────────────────────────────────────────────────────

case "${1:-help}" in
    build)          cmd_build ;;
    start)          cmd_start ;;
    stop)           cmd_stop ;;
    restart)        cmd_restart ;;
    status)         cmd_status ;;
    clean)          cmd_clean ;;
    submit)         shift; cmd_submit "$@" ;;
    next)           cmd_next ;;
    result)         shift; cmd_result "$@" ;;
    list|ls)        shift; cmd_list "$@" ;;
    job-status)     shift; cmd_job_status "$@" ;;
    job-log)        shift; cmd_job_log "$@" ;;
    metric-summary) cmd_metric_summary ;;
    sandbox-status) cmd_sandbox_status ;;
    test)           cmd_test ;;
    logs)           shift; cmd_logs "$@" ;;
    demo)           cmd_demo ;;
    help|--help|-h) cmd_help ;;
    *)              err "Unknown command: $1"; cmd_help; exit 1 ;;
esac
