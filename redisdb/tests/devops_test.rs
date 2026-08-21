use std::sync::Arc;

use tokio::sync::RwLock;

use redisops::handler;
use redisops::proto;
use redisops::store::Store;

#[tokio::test]
async fn test_job_submit_and_status() {
    let store = Store::new();
    let auth = Arc::new(RwLock::new(handler::AuthConfig::default()));

    // Submit a job
    let resp = handler::dispatch(
        store.clone(),
        vec![
            "job submit".into(),
            "job-001".into(),
            "smoke-test".into(),
            "qemu".into(),
            "make test".into(),
        ],
        auth.clone(),
    )
    .await;
    assert_eq!(resp[0], proto::SER_STR);
    let len = u32::from_le_bytes(resp[1..5].try_into().unwrap()) as usize;
    let id = String::from_utf8(resp[5..5 + len].to_vec()).unwrap();
    assert_eq!(id, "job-001");

    // Check status
    let resp = handler::dispatch(store.clone(), vec!["job status".into(), "job-001".into()], auth.clone()).await;
    assert_eq!(resp[0], proto::SER_STR);
    let len = u32::from_le_bytes(resp[1..5].try_into().unwrap()) as usize;
    let status = String::from_utf8(resp[5..5 + len].to_vec()).unwrap();
    assert_eq!(status, "queued");
}

#[tokio::test]
async fn test_job_next() {
    let store = Store::new();
    let auth = Arc::new(RwLock::new(handler::AuthConfig::default()));

    // Submit two jobs
    handler::dispatch(
        store.clone(),
        vec![
            "job submit".into(),
            "j1".into(),
            "test-a".into(),
            "qemu".into(),
            "run-a".into(),
        ],
        auth.clone(),
    )
    .await;
    handler::dispatch(
        store.clone(),
        vec![
            "job submit".into(),
            "j2".into(),
            "test-b".into(),
            "renode".into(),
            "run-b".into(),
        ],
        auth.clone(),
    )
    .await;

    // Pop first job (FIFO)
    let resp = handler::dispatch(store.clone(), vec!["job next".into()], auth.clone()).await;
    assert_eq!(resp[0], proto::SER_ARR);
    // Should be j1 (submitted first)
    let resp2 = handler::dispatch(store.clone(), vec!["job next".into()], auth.clone()).await;
    assert_eq!(resp2[0], proto::SER_ARR);
}

#[tokio::test]
async fn test_job_result() {
    let store = Store::new();
    let auth = Arc::new(RwLock::new(handler::AuthConfig::default()));

    handler::dispatch(
        store.clone(),
        vec![
            "job submit".into(),
            "j1".into(),
            "test".into(),
            "qemu".into(),
            "make".into(),
        ],
        auth.clone(),
    )
    .await;

    // Record result
    let resp = handler::dispatch(
        store.clone(),
        vec!["job result".into(), "j1".into(), "0".into(), "1500".into()],
        auth.clone(),
    )
    .await;
    assert_eq!(resp[0], proto::SER_STR);
    let len = u32::from_le_bytes(resp[1..5].try_into().unwrap()) as usize;
    let status = String::from_utf8(resp[5..5 + len].to_vec()).unwrap();
    assert_eq!(status, "passed");
}

#[tokio::test]
async fn test_metric_record() {
    let store = Store::new();
    let auth = Arc::new(RwLock::new(handler::AuthConfig::default()));

    let resp = handler::dispatch(
        store.clone(),
        vec![
            "metric record".into(),
            "test_duration".into(),
            "1.23".into(),
        ],
        auth.clone(),
    )
    .await;
    assert_eq!(resp[0], proto::SER_INT);
    let n = i64::from_le_bytes(resp[1..9].try_into().unwrap());
    assert_eq!(n, 1);
}

#[tokio::test]
async fn test_metric_summary() {
    let store = Store::new();
    let auth = Arc::new(RwLock::new(handler::AuthConfig::default()));

    // Record some metrics
    handler::dispatch(
        store.clone(),
        vec!["metric record".into(), "latency".into(), "0.5".into()],
        auth.clone(),
    )
    .await;

    let resp = handler::dispatch(store.clone(), vec!["metric summary".into()], auth.clone()).await;
    assert_eq!(resp[0], proto::SER_STR);
    let len = u32::from_le_bytes(resp[1..5].try_into().unwrap()) as usize;
    let summary = String::from_utf8(resp[5..5 + len].to_vec()).unwrap();
    assert!(summary.contains("metrics=1"));
}

#[tokio::test]
async fn test_sandbox_lifecycle() {
    let store = Store::new();
    let auth = Arc::new(RwLock::new(handler::AuthConfig::default()));

    // Register sandbox
    let resp = handler::dispatch(
        store.clone(),
        vec![
            "sandbox register".into(),
            "sb-1".into(),
            "qemu".into(),
            "127.0.0.1:4444".into(),
        ],
        auth.clone(),
    )
    .await;
    assert_eq!(resp[0], proto::SER_STR);

    // Check status
    let resp = handler::dispatch(store.clone(), vec!["sandbox status".into()], auth.clone()).await;
    assert_eq!(resp[0], proto::SER_ARR);
}

#[tokio::test]
async fn test_bitfield_ops() {
    let store = Store::new();
    let auth = Arc::new(RwLock::new(handler::AuthConfig::default()));

    // Set bit 0
    handler::dispatch(
        store.clone(),
        vec!["setbit".into(), "flags".into(), "0".into()],
        auth.clone(),
    )
    .await;
    let resp = handler::dispatch(
        store.clone(),
        vec!["getbit".into(), "flags".into(), "0".into()],
        auth.clone(),
    )
    .await;
    assert_eq!(resp[0], proto::SER_INT);
    let n = i64::from_le_bytes(resp[1..9].try_into().unwrap());
    assert_eq!(n, 1);

    // Set bit 7
    handler::dispatch(
        store.clone(),
        vec!["setbit".into(), "flags".into(), "7".into()],
        auth.clone(),
    )
    .await;
    let resp = handler::dispatch(store.clone(), vec!["bitcount".into(), "flags".into()], auth.clone()).await;
    let n = i64::from_le_bytes(resp[1..9].try_into().unwrap());
    assert_eq!(n, 2);

    // Clear bit 0
    handler::dispatch(
        store.clone(),
        vec!["clearbit".into(), "flags".into(), "0".into()],
        auth.clone(),
    )
    .await;
    let resp = handler::dispatch(
        store.clone(),
        vec!["getbit".into(), "flags".into(), "0".into()],
        auth.clone(),
    )
    .await;
    let n = i64::from_le_bytes(resp[1..9].try_into().unwrap());
    assert_eq!(n, 0);
}

#[tokio::test]
async fn test_list_ops() {
    let store = Store::new();
    let auth = Arc::new(RwLock::new(handler::AuthConfig::default()));

    // Push items
    handler::dispatch(store.clone(), vec!["lpush".into(), "q".into(), "a".into()], auth.clone()).await;
    handler::dispatch(store.clone(), vec!["lpush".into(), "q".into(), "b".into()], auth.clone()).await;

    // Length
    let resp = handler::dispatch(store.clone(), vec!["llen".into(), "q".into()], auth.clone()).await;
    let n = i64::from_le_bytes(resp[1..9].try_into().unwrap());
    assert_eq!(n, 2);

    // Pop from left (should be "b" since lpush inserts at front)
    let resp = handler::dispatch(store.clone(), vec!["lpop".into(), "q".into()], auth.clone()).await;
    assert_eq!(resp[0], proto::SER_STR);
    let len = u32::from_le_bytes(resp[1..5].try_into().unwrap()) as usize;
    let val = String::from_utf8(resp[5..5 + len].to_vec()).unwrap();
    assert_eq!(val, "b");

    // Range
    handler::dispatch(store.clone(), vec!["lpush".into(), "q".into(), "c".into()], auth.clone()).await;
    let resp = handler::dispatch(
        store.clone(),
        vec!["lrange".into(), "q".into(), "0".into(), "-1".into()],
        auth.clone(),
    )
    .await;
    assert_eq!(resp[0], proto::SER_ARR);
}

#[tokio::test]
async fn test_job_list() {
    let store = Store::new();
    let auth = Arc::new(RwLock::new(handler::AuthConfig::default()));

    handler::dispatch(
        store.clone(),
        vec![
            "job submit".into(),
            "j1".into(),
            "t1".into(),
            "qemu".into(),
            "run".into(),
        ],
        auth.clone(),
    )
    .await;
    handler::dispatch(
        store.clone(),
        vec![
            "job submit".into(),
            "j2".into(),
            "t2".into(),
            "renode".into(),
            "run".into(),
        ],
        auth.clone(),
    )
    .await;

    let resp = handler::dispatch(store.clone(), vec!["job list".into()], auth.clone()).await;
    assert_eq!(resp[0], proto::SER_ARR);
}
