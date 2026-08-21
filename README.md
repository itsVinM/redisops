# RedisOps

A Redis-compatible state store with B-tree storage, time-series DB, and job orchestration.

## Quick Start

```bash
cd redisdb && cargo run
redis-cli -p 6379
```

## Features

- **KV Store**: `SET`, `GET`, `DEL`, `EXPIRE`, `TTL`
- **Lists**: `LPUSH`, `RPOP`, `LRANGE`, `LINDEX`, `LLEN`
- **Bitfields**: `SETBIT`, `GETBIT`, `BITCOUNT`, `BFGET`, `BFSET`
- **Sorted Sets**: `ZADD`, `ZREM`, `ZSCORE`, `ZQUERY`
- **Time-Series**: `TS.ADD`, `TS.RANGE`, `TS.INFO`, `TS.LIST`
- **Job Queue**: `JOB.SUBMIT`, `JOB.NEXT`, `JOB.RESULT`, `JOB.STATUS`
- **Metrics**: `METRIC.RECORD`, `METRIC.QUERY`, `METRIC.SUMMARY`
- **Sandbox**: `SANDBOX.REGISTER`, `SANDBOX.CLAIM`, `SANDBOX.RELEASE`
- **Storage**: Custom B-tree with buffer pool + WAL

## Testing

```bash
# Unit tests (37 tests)
cargo test

# CI checks (format + clippy + tests + build)
bash ci.sh
```

## Project Structure

```
redis-rs/
├── redisdb/          # Rust server
│   ├── src/btree/    # B-tree storage engine
│   ├── src/tsdb/     # Time-series database
│   ├── src/handler.rs # Command dispatch
│   └── tests/
├── sandbox/          # C++20 sandbox runtime
├── ci.sh            # CI script
└── devops.sh        # Orchestration
```
