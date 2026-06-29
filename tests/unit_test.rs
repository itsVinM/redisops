use redis_rs::handler;
use redis_rs::proto;
use redis_rs::store::Store;

#[tokio::test]
async fn test_handler_set_get() {
    let store = Store::new();

    let resp = handler::dispatch(store.clone(), vec!["set".into(), "k".into(), "v".into()]).await;
    assert_eq!(resp[0], proto::SER_NIL);

    let resp = handler::dispatch(store.clone(), vec!["get".into(), "k".into()]).await;
    assert_eq!(resp[0], proto::SER_STR);
    let len = u32::from_le_bytes(resp[1..5].try_into().unwrap()) as usize;
    let val = String::from_utf8(resp[5..5 + len].to_vec()).unwrap();
    assert_eq!(val, "v");
}

#[tokio::test]
async fn test_handler_del() {
    let store = Store::new();

    handler::dispatch(store.clone(), vec!["set".into(), "k".into(), "v".into()]).await;
    let resp = handler::dispatch(store.clone(), vec!["del".into(), "k".into()]).await;
    assert_eq!(resp[0], proto::SER_INT);
    let n = i64::from_le_bytes(resp[1..9].try_into().unwrap());
    assert_eq!(n, 1);

    let resp = handler::dispatch(store.clone(), vec!["del".into(), "k".into()]).await;
    let n = i64::from_le_bytes(resp[1..9].try_into().unwrap());
    assert_eq!(n, 0);
}

#[tokio::test]
async fn test_handler_zadd_zscore() {
    let store = Store::new();

    let resp = handler::dispatch(
        store.clone(),
        vec!["zadd".into(), "z".into(), "3.14".into(), "pi".into()],
    )
    .await;
    assert_eq!(resp[0], proto::SER_INT);
    let n = i64::from_le_bytes(resp[1..9].try_into().unwrap());
    assert_eq!(n, 1);

    let resp = handler::dispatch(
        store.clone(),
        vec!["zscore".into(), "z".into(), "pi".into()],
    )
    .await;
    assert_eq!(resp[0], proto::SER_DBL);
    let val = f64::from_le_bytes(resp[1..9].try_into().unwrap());
    assert!((val - 3.14).abs() < 1e-9);
}

#[tokio::test]
async fn test_handler_unknown() {
    let store = Store::new();
    let resp = handler::dispatch(store.clone(), vec!["nosuch".into()]).await;
    assert_eq!(resp[0], proto::SER_ERR);
}

#[tokio::test]
async fn test_handler_arity() {
    let store = Store::new();
    let resp = handler::dispatch(store.clone(), vec!["get".into()]).await;
    assert_eq!(resp[0], proto::SER_ERR);
}
