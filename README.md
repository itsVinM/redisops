# foundry

Validation & orchestration platform. Redis state store in Rust, sandboxed execution in C++20.

## What

A self-hosted CI system for hardware validation. Submit test jobs, run them in isolated sandboxes (Linux namespaces + cgroups + seccomp), collect metrics, deploy firmware.

```
┌──────────┐     ┌───────────┐     ┌──────────────┐
│   CLI    │────►│  Redis-rs │────►│ C++20 Sandbox│
│ (submit, │     │  (state,  │     │ (clone,      │
│  status) │     │  queue,   │     │  cgroup,     │
│          │     │  metrics) │     │  seccomp)    │
└──────────┘     └───────────┘     └──────────────┘
```

## Quick Start

```bash
./devops.sh build       # build Rust + C++
./devops.sh start       # start Redis server + sandbox
./devops.sh submit test "echo hello"  # submit a job
./devops.sh list        # list all jobs
./devops.sh status      # check system status
./devops.sh stop        # stop everything
```

## Commands

### System
| Command | Description |
|---------|-------------|
| `./devops.sh build` | Build Rust server + C++20 sandbox |
| `./devops.sh start` | Start Redis server + sandbox |
| `./devops.sh stop` | Stop all services |
| `./devops.sh status` | Show system status |
| `./devops.sh test` | Run all tests (Rust + C++) |

### Jobs
| Command | Description |
|---------|-------------|
| `./devops.sh submit <name> <cmd> [target]` | Submit a validation job |
| `./devops.sh next` | Fetch next job from queue |
| `./devops.sh result <id> <exit> <ms>` | Record job result |
| `./devops.sh list [status]` | List jobs |

### Redis Commands (added)
| Command | Description |
|---------|-------------|
| `JOB SUBMIT/NEXT/STATUS/RESULT/LOG/LIST` | Job queue management |
| `METRIC RECORD/QUERY/SUMMARY` | Time-series metrics |
| `SANDBOX REGISTER/CLAIM/RELEASE/STATUS` | Sandbox lifecycle |
| `SETBIT/GETBIT/BITCOUNT/BFGET/BFSET` | Bitfield operations |
| `LPUSH/LPOP/RPOP/LRANGE/LLEN/LREM` | List/queue primitives |

## Architecture

```
redis-rs/
├── src/                    # Rust: Redis server + DevOps commands
│   ├── main.rs             # Entry point
│   ├── server.rs           # TCP listener, connection pool
│   ├── handler.rs          # Command dispatch (SET/GET/JOB/METRIC/SANDBOX)
│   ├── store/mod.rs        # KV store + Lists + Bitfields + TTL
│   ├── proto/mod.rs        # Binary wire protocol
│   └── stats.rs            # Connection/command stats
├── sandbox/                # C++20: Isolated execution runtime
│   ├── include/devops/     # Headers
│   │   ├── redis_client.hpp
│   │   ├── sandbox.hpp
│   │   ├── cgroup.hpp
│   │   └── seccomp.hpp
│   └── src/                # Implementation
│       ├── main.cpp        # Orchestrator loop
│       ├── redis_client.cpp
│       ├── sandbox.cpp     # Linux namespace isolation
│       ├── cgroup.cpp      # cgroups v2 limits
│       └── seccomp.cpp     # Syscall filtering
├── tests/                  # Rust tests (37 passing)
└── devops.sh               # Orchestration script
```

## Tests

```bash
# All 37 tests (28 original + 9 DevOps)
cargo test

# Build & run everything
./devops.sh test
```

## Docker

```bash
USE_DOCKER=true ./devops.sh build --docker
USE_DOCKER=true ./devops.sh start
```

## Wire Protocol

Binary (not RESP): `[4B msg_len][4B n_args]([4B arg_len][arg_bytes])*`

Response: `[4B resp_len][tag_byte][data...]`
