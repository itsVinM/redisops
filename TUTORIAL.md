# Build a Redis-alike from Scratch in Rust

A step-by-step tutorial on building a key-value server with tokio async I/O,
binary protocol, sorted sets, TTL expiry, and a live TUI dashboard.

---

## Step 1: Project Setup

```bash
cargo new redis-rs
cd redis-rs
```

Add tokio with full features to `Cargo.toml`:

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

---

## Step 2: Binary Wire Protocol (`src/proto/mod.rs`)

We need a way to encode requests and responses over TCP. Redis uses RESP (Redis
Serialization Protocol), but we'll use a simpler binary format:

**Request format:** `[4B payload_len][4B num_args]([4B arg_len][arg_bytes])*`

**Response format:** `[4B payload_len][1B type_tag][payload]`

Where type_tag is one of:
- `0` NIL
- `1` ERR
- `2` STR
- `3` INT
- `4` DBL (double/float)
- `5` ARR

We define a `Response` enum and implement `encode()`:

```rust
pub enum Response {
    Nil,
    Err { code: i32, msg: String },
    Str(String),
    Int(i64),
    Dbl(f64),
    Arr(Vec<Response>),
}
```

Key functions:
- `read_request(r) -> Vec<String>` — reads 4-byte length, then that many bytes,
  parses num_args, then reads each arg by length-prefix.
- `write_response(w, payload)` — writes 4-byte length prefix followed by payload.

The trait bounds `R: AsyncRead + Unpin` allow reading from any async source
(TcpStream, byte slices, etc.).

**Why length-prefixed framing?** TCP is a stream protocol — there are no message
boundaries. `read_exact` reads exactly N bytes, so we always know when a
complete message has arrived. Without length prefixing, the server wouldn't know
where one request ends and the next begins.

---

## Step 3: Sorted Set (`src/zset/mod.rs`)

A sorted set associates members (strings) with scores (f64) and allows
querying by score range. We need two data structures for O(log n) operations:

- **`HashMap<String, f64>`** (`by_name`) — maps member → score for O(1) lookup
- **Sorted `Vec<Entry>`** (`sorted`) — entries ordered by (score, name) for
  binary search O(log n) insertion and query

Insertion finds the correct position via `binary_search_by`, then inserts at
that index (O(n) shift). This is fast enough for moderate sizes and much simpler
than a balanced BST or skiplist that Redis uses.

```rust
pub fn add(&mut self, name: String, score: f64) -> bool {
    if let Some(&old) = self.by_name.get(&name) {
        if (old - score).abs() < f64::EPSILON { return false; }
        self.remove(&name);  // remove old entry if score changed
    }
    self.by_name.insert(name.clone(), score);
    let e = Entry { name, score };
    let i = self.sorted.binary_search_by(|probe| ... );
    self.sorted.insert(i, e);
    true
}
```

The `query(min_score, min_name, offset, limit)` method finds the first entry
>= `(min_score, min_name)` via binary search, then slices the vec.

---

## Step 4: Thread-safe Store (`src/store/mod.rs`)

The store is shared across all connections. In Rust, shared mutable state
requires synchronization:

- **`std::sync::RwLock`** — blocks the thread on contention
- **`tokio::sync::RwLock`** — yields the async task on contention

We use `tokio::sync::RwLock` because our handler is async and shouldn't block
the tokio worker thread. The store is wrapped in `Arc<RwLock<HashMap<...>>>`:

```rust
pub struct Store { inner: Arc<RwLock<Inner>> }

struct Inner { data: HashMap<String, Entry> }
```

Each entry stores its type (`Str` or `ZSet`) and optional expiry time:

```rust
struct Entry {
    typ: Value,
    exp_at: Option<Instant>,
}
```

**TTL expiry** — a background task periodically scans and removes expired keys:

```rust
pub fn new_with_expiry() -> (Self, watch::Sender<()>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                _ = interval.tick() => inner.data.retain(|_, e| !e.expired()),
                _ = rx.changed() => break,  // shutdown signal
            }
        }
    });
}
```

This avoids checking expiry on every read — expired keys are cleaned up
asynchronously. Reads still check expiry on access for immediate consistency.

---

## Step 5: Command Handler (`src/handler.rs`)

The handler receives parsed args and dispatches to the store. It's a simple
`match` on the command name:

```rust
pub async fn dispatch(store: Store, args: Vec<String>) -> Vec<u8> {
    let cmd = args[0].to_lowercase();
    match cmd.as_str() {
        "set" => { store.set_str(args[1].clone(), args[2].clone()).await;
                   Response::Nil.encode() }
        "get" => { ... }
        "zadd" => { ... }
        _ => err_unknown("unknown command"),
    }
}
```

Each arm:
1. Validates arity (number of arguments)
2. Calls the store method
3. Encodes the response

Errors return encoded `Response::Err` with a code and human-readable message.

---

## Step 6: TCP Server (`src/server.rs`)

The server accepts connections in a loop and spawns a task per connection:

```rust
pub async fn serve(self, listener: TcpListener) -> Result<...> {
    loop {
        let permit = semaphore.acquire_owned().await?;
        let (stream, peer) = listener.accept().await?;
        tokio::spawn(async move {
            handle_conn(stream, store, timeout).await;
            drop(permit);
        });
    }
}
```

**Connection limiting** — a `Semaphore` limits concurrent connections. Each
accepted connection acquires a permit; dropping it releases the slot.

**Per-connection handler:**
```rust
async fn handle_conn(mut stream: TcpStream, store: Store, timeout: Duration) {
    loop {
        let args = tokio::time::timeout(timeout, read_request(&mut stream)).await;
        let args = args??;  // error or timeout → close connection
        let resp = dispatch(store.clone(), args).await;
        write_response(&mut stream, &resp).await?;
    }
}
```

The `read_timeout` prevents hung connections. After timeout, the connection is
closed and the permit released.

`set_nodelay(true)` disables Nagle's algorithm for lower latency.

---

## Step 7: Graceful Shutdown (`src/main.rs`)

`tokio::signal::ctrl_c()` allows clean shutdown on SIGINT/SIGTERM:

```rust
#[tokio::main]
async fn main() {
    let srv = Server::new(Config::default());
    tokio::select! {
        res = srv.listen_and_serve() => { ... }
        _ = tokio::signal::ctrl_c() => { info!("shutting down"); }
    }
}
```

When the user presses Ctrl-C, `listen_and_serve` is dropped, which drops the
`TcpListener`, stopping new connections. Existing connections continue until
they close naturally.

**Note:** This doesn't forcefully kill connections — they drain gracefully.

---

## Step 8: Live TUI Dashboard (`src/bin/redis-tui.rs`)

Using `ratatui` + `crossterm`, we build a real-time terminal dashboard.

**Stats** (`src/stats.rs`) — atomic counters shared between server and TUI:

```rust
pub struct Stats {
    total_connections: AtomicU64,
    active_connections: AtomicI64,
    total_commands: AtomicU64,
    last_cmds: Mutex<VecDeque<String>>,  // ring buffer of last 64 commands
    started_at: Instant,
}
```

The server increments stats in `handle_conn`:
- `stats.inc_conn()` on accept
- `stats.dec_conn()` when connection closes
- `stats.inc_cmd(cmd_name)` on each command

**TUI loop** — polls stats every 100ms, checks `shutdown_rx` for server errors:

```rust
loop {
    terminal.draw(|f| ui(f, &stats.snapshot()))?;
    if *shutdown_rx.borrow() { break; }
    if event::poll(Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            if key.code == KeyCode::Char('q') { break; }
        }
    }
}
```

The TUI binary (`redis-tui`) spawns the server in a background `tokio::spawn`
task and runs the dashboard on the main thread. One binary, no separate process.

---

## Step 9: Testing

Three test files:

1. **`tests/unit_test.rs`** — tests the handler/dispatch logic directly without
   TCP. Creates a Store, calls `dispatch()`, checks encoded responses.

2. **`tests/integration_test.rs`** — tests the handler + store logic via TCP.
   Uses `TcpStream` to connect to a server spawned in the test runtime.

3. **`tests/tcp_integration_test.rs`** — end-to-end tests with full server
   stack (listener, accept, dispatch, write). Tests the actual wire protocol.

**Key lesson learned:** Always match framing. The server initially wrote raw
`Response::encode()` bytes without the 4-byte length prefix. The client
expected the prefix. After changing `stream.write_all(&resp)` to
`proto::write_response(&mut stream, &resp)`, all tests passed.

**Docker for consistency:** macOS has a known issue with same-process TCP
loopback in tokio (kqueue/reactor interaction). Running tests in Docker (Linux)
avoids this. The Dockerfile uses `rust:slim-bookworm`.

---

## Project Structure

```
src/
├── main.rs          # Entry point, tokio::select! shutdown
├── lib.rs           # Module declarations
├── server.rs        # TcpListener, accept loop, connection pool
├── handler.rs       # Command dispatch match
├── proto/mod.rs     # Binary wire protocol
├── store/mod.rs     # RwLock<HashMap> + TTL background task
├── zset/mod.rs      # Sorted set (HashMap + binary search)
├── stats.rs         # Atomic counters for TUI
└── bin/
    └── redis-tui.rs # ratatui dashboard
```

## Wire Protocol Reference

```
Request:
  [4B payload_len][4B num_args]([4B arg_len][arg_bytes])*

Response:
  [4B payload_len][1B type_tag][payload]

type_tag:
  0=NIL  1=ERR  2=STR  3=INT  4=DBL  5=ARR

STRING payload:  [type=2][4B len][bytes]
INT payload:     [type=3][8B i64 LE]
DBL payload:     [type=4][8B f64 LE]
ARR payload:     [type=5][4B count]([sub-response])*
ERR payload:     [type=1][4B code][4B msg_len][bytes]
NIL payload:     [type=0]  (0 bytes after type)
```
