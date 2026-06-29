# redis-rs

A minimal Redis-alike key-value store in Rust, built with tokio async I/O. Supports strings, sorted sets, TTL expiry, and a live TUI dashboard.

## Wire Protocol

Binary format (not RESP): `[4B payload_len][4B num_args]([4B arg_len][arg_bytes])*`

## Commands

| Command | Example | Description |
|---------|---------|-------------|
| `SET` | `SET key value` | Store a string |
| `GET` | `GET key` | Retrieve a string |
| `DEL` | `DEL key` | Delete a key |
| `KEYS` | `KEYS` | List all keys |
| `PEXPIRE` | `PEXPIRE key 5000` | Set TTL in ms |
| `PTTL` | `PTTL key` | Get remaining TTL |
| `ZADD` | `ZADD zset 1.5 member` | Add to sorted set |
| `ZREM` | `ZREM zset member` | Remove from sorted set |
| `ZSCORE` | `ZSCORE zset member` | Get score |
| `ZQUERY` | `ZQUERY zset 0 "" 0 10` | Query by score range |

## Run

```bash
# Server only
cargo run --bin redis-rs

# Server + TUI dashboard
cargo run --bin redis-tui
```

## TUI Dashboard

```
┌────────────────────────────────────────────────────────────┐
│ redis-rs Monitor    127.0.0.1:1234    up 2m 13s            │
├──────────────────────┬─────────────────────────────────────┤
│ Connections          │  Recent Commands                     │
│   Active:       2    │  SET k v                             │
│   Total:       15    │  GET k                               │
│                      │  ZADD z 1.5 a                        │
│ Commands             │  KEYS                                │
│   Processed:   87    │  DEL x                               │
│                      │  ZSCORE z a                          │
│ Keys                 │  ZQUERY z 0 "" 0 10                  │
│   Stored keys: ?     │                                      │
└──────────────────────┴──────────────────────────────────────┘
```

Commands are color-coded: `SET`/`DEL` yellow, `GET` green, `Z*` cyan. Press `q` or `Esc` to quit.

## Architecture

```
src/
├── main.rs          # Entry point, ctrl-c shutdown
├── lib.rs           # Module registry
├── server.rs        # TCP listener, connection pool, stats wiring
├── handler.rs       # Command dispatch (SET/GET/DEL/KEYS/Z*)
├── proto/mod.rs     # Binary wire protocol encode/decode
├── store/mod.rs     # Thread-safe KV store with RwLock + TTL
├── zset/mod.rs      # Sorted set (HashMap + sorted Vec, binary search)
├── stats.rs         # Atomic counters for TUI dashboard
└── bin/redis-tui.rs # ratatui terminal dashboard
```

## Test

```bash
# Unit + integration tests (all 28 pass)
docker build -t redis-rs . && docker run --rm redis-rs cargo test
```
