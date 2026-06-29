use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;

use redis_rs::{handler, proto, store::Store};

fn make_frame(args: &[&str]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&(args.len() as u32).to_le_bytes());
    for arg in args {
        payload.extend_from_slice(&(arg.len() as u32).to_le_bytes());
        payload.extend_from_slice(arg.as_bytes());
    }
    let mut frame = Vec::new();
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    frame
}

async fn expect_response(client: &mut TcpStream) -> Vec<u8> {
    let mut len_buf = [0u8; 4];
    client.read_exact(&mut len_buf).await.unwrap();
    let payload_len = u32::from_le_bytes(len_buf) as usize;
    let mut resp = vec![0u8; payload_len];
    if payload_len > 0 {
        client.read_exact(&mut resp).await.unwrap();
    }
    resp
}

async fn spawn_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let store = Store::new();

    tokio::spawn(async move {
        let (mut stream, _peer) = listener.accept().await.unwrap();
        stream.set_nodelay(true).unwrap();
        loop {
            let args = match tokio::time::timeout(
                Duration::from_secs(5),
                proto::read_request(&mut stream),
            )
            .await
            {
                Ok(Ok(a)) => a,
                _ => return,
            };
            let resp = handler::dispatch(store.clone(), args).await;
            proto::write_response(&mut stream, &resp).await.unwrap();
        }
    });

    sleep(Duration::from_millis(200)).await;
    addr
}

#[tokio::test]
async fn test_set_get() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(&addr).await.unwrap();
    client.set_nodelay(true).unwrap();

    // SET
    let frame = make_frame(&["set", "k", "v"]);
    client.write_all(&frame).await.unwrap();
    let resp = expect_response(&mut client).await;
    assert_eq!(resp[0], 0, "SET should return NIL");

    // GET
    let frame = make_frame(&["get", "k"]);
    client.write_all(&frame).await.unwrap();
    let resp = expect_response(&mut client).await;
    assert_eq!(resp[0], 2, "GET should return STR");
    let slen = u32::from_le_bytes(resp[1..5].try_into().unwrap()) as usize;
    let val = String::from_utf8(resp[5..5 + slen].to_vec()).unwrap();
    assert_eq!(val, "v");
}

#[tokio::test]
async fn test_del() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(&addr).await.unwrap();
    client.set_nodelay(true).unwrap();

    // SET
    client.write_all(&make_frame(&["set", "x", "1"])).await.unwrap();
    expect_response(&mut client).await;

    // DEL
    client.write_all(&make_frame(&["del", "x"])).await.unwrap();
    let resp = expect_response(&mut client).await;
    assert_eq!(resp[0], 3, "DEL should return INT");

    // GET deleted key
    client.write_all(&make_frame(&["get", "x"])).await.unwrap();
    let resp = expect_response(&mut client).await;
    assert_eq!(resp[0], 0, "GET of deleted key should return NIL");
}

#[tokio::test]
async fn test_zset() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(&addr).await.unwrap();
    client.set_nodelay(true).unwrap();

    // ZADD
    client.write_all(&make_frame(&["zadd", "z", "1.5", "a"])).await.unwrap();
    let resp = expect_response(&mut client).await;
    assert_eq!(resp[0], 3, "ZADD should return INT");

    // ZSCORE
    client.write_all(&make_frame(&["zscore", "z", "a"])).await.unwrap();
    let resp = expect_response(&mut client).await;
    assert_eq!(resp[0], 4, "ZSCORE should return DBL");
    let score = f64::from_le_bytes(resp[1..9].try_into().unwrap());
    assert!((score - 1.5).abs() < 1e-10);

    // ZQUERY: min_score=0, min_name="", offset=0, limit=10
    client.write_all(&make_frame(&["zquery", "z", "0", "", "0", "10"])).await.unwrap();
    let resp = expect_response(&mut client).await;
    assert_eq!(resp[0], 5, "ZQUERY should return ARR");
}

#[tokio::test]
async fn test_keys() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(&addr).await.unwrap();
    client.set_nodelay(true).unwrap();

    client.write_all(&make_frame(&["set", "a", "1"])).await.unwrap();
    expect_response(&mut client).await;
    client.write_all(&make_frame(&["set", "b", "2"])).await.unwrap();
    expect_response(&mut client).await;

    client.write_all(&make_frame(&["keys"])).await.unwrap();
    let resp = expect_response(&mut client).await;
    assert_eq!(resp[0], 5, "KEYS should return ARR");
}

#[tokio::test]
async fn test_unknown_command() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(&addr).await.unwrap();
    client.set_nodelay(true).unwrap();

    client.write_all(&make_frame(&["bogus"])).await.unwrap();
    let resp = expect_response(&mut client).await;
    assert_eq!(resp[0], 1, "unknown command should return ERR");
    // ERR: [tag(1)][code:4B][msg_len:4B][msg]
    let slen = u32::from_le_bytes(resp[5..9].try_into().unwrap()) as usize;
    let msg = String::from_utf8(resp[9..9 + slen].to_vec()).unwrap();
    assert!(msg.contains("unknown"), "error should mention 'unknown'");
}
