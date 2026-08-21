use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Starts the real server on a random port and returns the address.
async fn start_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();

    let cfg = redisops::server::Config {
        addr: addr.clone(),
        ..Default::default()
    };
    let srv = redisops::server::Server::new(cfg);

    tokio::spawn(async move {
        let _ = srv.serve(listener).await;
    });

    tokio::time::sleep(Duration::from_millis(10)).await;
    addr
}

struct TestClient {
    stream: TcpStream,
}

impl TestClient {
    async fn connect(addr: &str) -> Self {
        let stream = TcpStream::connect(addr).await.unwrap();
        TestClient { stream }
    }

    async fn send(&mut self, args: &[&str]) {
        let mut payload = Vec::new();
        payload.extend_from_slice(&(args.len() as u32).to_le_bytes());
        for arg in args {
            payload.extend_from_slice(&(arg.len() as u32).to_le_bytes());
            payload.extend_from_slice(arg.as_bytes());
        }

        let mut frame = Vec::new();
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&payload);
        self.stream.write_all(&frame).await.unwrap();
    }

    async fn recv_raw(&mut self) -> Vec<u8> {
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await.unwrap();
        let payload_len = u32::from_le_bytes(len_buf) as usize;
        let mut payload = vec![0u8; payload_len];
        if payload_len > 0 {
            self.stream.read_exact(&mut payload).await.unwrap();
        }
        payload
    }

    async fn send_recv(&mut self, args: &[&str]) -> Vec<u8> {
        self.send(args).await;
        self.recv_raw().await
    }

    async fn send_recv_tag(&mut self, expected_tag: u8, args: &[&str]) -> Vec<u8> {
        let raw = self.send_recv(args).await;
        assert!(!raw.is_empty(), "expected non-empty response");
        assert_eq!(
            raw[0], expected_tag,
            "tag mismatch: got {} want {}",
            raw[0], expected_tag
        );
        raw
    }

    async fn must_send_recv(&mut self, expected_tag: u8, args: &[&str]) -> Vec<u8> {
        self.send_recv_tag(expected_tag, args).await
    }
}

fn decode_str(raw: &[u8]) -> String {
    let len = u32::from_le_bytes(raw[1..5].try_into().unwrap()) as usize;
    String::from_utf8(raw[5..5 + len].to_vec()).unwrap()
}

fn decode_int(raw: &[u8]) -> i64 {
    i64::from_le_bytes(raw[1..9].try_into().unwrap())
}

fn decode_dbl(raw: &[u8]) -> f64 {
    f64::from_le_bytes(raw[1..9].try_into().unwrap())
}

#[tokio::test]
async fn test_set_get() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(0, &["set", "hello", "world"]).await;
    let raw = c.must_send_recv(2, &["get", "hello"]).await;
    assert_eq!(decode_str(&raw), "world");
}

#[tokio::test]
async fn test_get_missing() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(0, &["get", "nosuchkey"]).await;
}

#[tokio::test]
async fn test_del() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(0, &["set", "k", "v"]).await;
    let raw = c.must_send_recv(3, &["del", "k"]).await;
    assert_eq!(decode_int(&raw), 1);
    let raw = c.must_send_recv(3, &["del", "k"]).await;
    assert_eq!(decode_int(&raw), 0);
}

#[tokio::test]
async fn test_keys() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(0, &["set", "a", "1"]).await;
    c.must_send_recv(0, &["set", "b", "2"]).await;
    let raw = c.must_send_recv(5, &["keys"]).await;
    let count = u32::from_le_bytes(raw[1..5].try_into().unwrap());
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_pexpire() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(0, &["set", "k", "v"]).await;
    let raw = c.must_send_recv(3, &["pexpire", "k", "500"]).await;
    assert_eq!(decode_int(&raw), 1);
}

#[tokio::test]
async fn test_pttl() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    let raw = c.must_send_recv(3, &["pttl", "no"]).await;
    assert_eq!(decode_int(&raw), -2);

    c.must_send_recv(0, &["set", "k", "v"]).await;
    let raw = c.must_send_recv(3, &["pttl", "k"]).await;
    assert_eq!(decode_int(&raw), -1);

    c.must_send_recv(3, &["pexpire", "k", "1000"]).await;
    let raw = c.must_send_recv(3, &["pttl", "k"]).await;
    let ttl = decode_int(&raw);
    assert!(ttl > 0 && ttl <= 1000, "ttl {} should be in (0, 1000]", ttl);
}

#[tokio::test]
async fn test_expiry() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(0, &["set", "k", "v"]).await;
    c.must_send_recv(3, &["pexpire", "k", "80"]).await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    c.must_send_recv(0, &["get", "k"]).await;
}

#[tokio::test]
async fn test_zadd() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    let raw = c.must_send_recv(3, &["zadd", "z", "1.5", "alice"]).await;
    assert_eq!(decode_int(&raw), 1);
    let raw = c.must_send_recv(3, &["zadd", "z", "1.5", "alice"]).await;
    assert_eq!(decode_int(&raw), 0);
}

#[tokio::test]
async fn test_zscore() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(3, &["zadd", "z", "3.14", "pi"]).await;
    let raw = c.must_send_recv(4, &["zscore", "z", "pi"]).await;
    let got = decode_dbl(&raw);
    assert!((got - 3.14).abs() < 1e-9, "zscore got {} want 3.14", got);
}

#[tokio::test]
async fn test_zscore_missing() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(0, &["zscore", "z", "nobody"]).await;
}

#[tokio::test]
async fn test_zrem() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(3, &["zadd", "z", "1.0", "a"]).await;
    let raw = c.must_send_recv(3, &["zrem", "z", "a"]).await;
    assert_eq!(decode_int(&raw), 1);
    let raw = c.must_send_recv(3, &["zrem", "z", "a"]).await;
    assert_eq!(decode_int(&raw), 0);
}

#[tokio::test]
async fn test_zquery() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    for (i, name) in ["a", "b", "c", "d", "e"].iter().enumerate() {
        c.must_send_recv(3, &["zadd", "z", &format!("{}", i + 1), name])
            .await;
    }
    let raw = c
        .must_send_recv(5, &["zquery", "z", "2", "", "0", "10"])
        .await;
    let count = u32::from_le_bytes(raw[1..5].try_into().unwrap());
    assert_eq!(count, 8);
}

#[tokio::test]
async fn test_unknown_command() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(1, &["nosuchcmd"]).await;
}

#[tokio::test]
async fn test_wrong_arity() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(1, &["get"]).await;
}

#[tokio::test]
async fn test_type_error() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(0, &["set", "k", "v"]).await;
    c.must_send_recv(1, &["zadd", "k", "1.0", "m"]).await;
}

#[tokio::test]
async fn test_bad_score() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(1, &["zadd", "z", "notanumber", "m"]).await;
}

#[tokio::test]
async fn test_bad_expiry() {
    let addr = start_server().await;
    let mut c = TestClient::connect(&addr).await;

    c.must_send_recv(0, &["set", "k", "v"]).await;
    c.must_send_recv(1, &["pexpire", "k", "notanint"]).await;
}

#[tokio::test]
async fn test_multiple_clients() {
    let addr = start_server().await;

    let mut handles = Vec::new();
    for i in 0..10 {
        let addr = addr.clone();
        handles.push(tokio::spawn(async move {
            let mut c = TestClient::connect(&addr).await;
            let key = format!("key{}", i);
            c.must_send_recv(0, &["set", &key, "val"]).await;
            let raw = c.must_send_recv(2, &["get", &key]).await;
            assert_eq!(decode_str(&raw), "val");
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}
