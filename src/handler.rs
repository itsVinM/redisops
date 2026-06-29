use crate::proto::{self, Response};
use crate::store::Store;

pub async fn dispatch(store: Store, args: Vec<String>) -> Vec<u8> {
    if args.is_empty() {
        return err_unknown("empty command");
    }

    let cmd = args[0].to_lowercase();
    let arity = args.len();

    // Each arm: validate arity then call handler.
    // Macros would be cleaner, but this keeps it explicit.
    match cmd.as_str() {
        "keys" => {
            if arity != 1 {
                return err_arg();
            }
            let keys = store.keys().await;
            let elems: Vec<Response> = keys.into_iter().map(Response::Str).collect();
            Response::Arr(elems).encode()
        }
        "get" => {
            if arity != 2 {
                return err_arg();
            }
            let (val, wrong_type) = store.get_str(&args[1]).await;
            if wrong_type {
                return err_type("expect string type");
            }
            match val {
                Some(v) => Response::Str(v).encode(),
                None => Response::Nil.encode(),
            }
        }
        "set" => {
            if arity != 3 {
                return err_arg();
            }
            store.set_str(args[1].clone(), args[2].clone()).await;
            Response::Nil.encode()
        }
        "del" => {
            if arity != 2 {
                return err_arg();
            }
            let ok = store.del(&args[1]).await;
            Response::Int(if ok { 1 } else { 0 }).encode()
        }
        "pexpire" => {
            if arity != 3 {
                return err_arg();
            }
            let ms: i64 = match args[2].parse() {
                Ok(v) => v,
                Err(_) => return err_arg_msg("expect int64"),
            };
            let ok = store.expire(&args[1], ms).await;
            Response::Int(if ok { 1 } else { 0 }).encode()
        }
        "pttl" => {
            if arity != 2 {
                return err_arg();
            }
            let ttl = store.ttl_ms(&args[1]).await;
            Response::Int(ttl).encode()
        }
        "zadd" => {
            if arity != 4 {
                return err_arg();
            }
            let score: f64 = match args[2].parse() {
                Ok(v) => v,
                Err(_) => return err_arg_msg("expect fp number"),
            };
            let (added, wrong_type) = store.z_add(args[1].clone(), args[3].clone(), score).await;
            if wrong_type {
                return err_type("expect zset");
            }
            Response::Int(if added { 1 } else { 0 }).encode()
        }
        "zrem" => {
            if arity != 3 {
                return err_arg();
            }
            let (removed, wrong_type) = store.z_rem(&args[1], &args[2]).await;
            if wrong_type {
                return err_type("expect zset");
            }
            Response::Int(if removed { 1 } else { 0 }).encode()
        }
        "zscore" => {
            if arity != 3 {
                return err_arg();
            }
            let (score, wrong_type) = store.z_score(&args[1], &args[2]).await;
            if wrong_type {
                return err_type("expect zset");
            }
            match score {
                Some(v) => Response::Dbl(v).encode(),
                None => Response::Nil.encode(),
            }
        }
        "zquery" => {
            if arity != 6 {
                return err_arg();
            }
            let score: f64 = match args[2].parse() {
                Ok(v) => v,
                Err(_) => return err_arg_msg("expect fp number"),
            };
            let offset: i64 = match args[4].parse() {
                Ok(v) => v,
                Err(_) => return err_arg_msg("expect int"),
            };
            let limit: i64 = match args[5].parse() {
                Ok(v) => v,
                Err(_) => return err_arg_msg("expect int"),
            };
            let (entries, wrong_type) = store.z_query(&args[1], score, &args[3], offset, limit).await;
            if wrong_type {
                return err_type("expect zset");
            }
            let elems: Vec<Response> = entries
                .into_iter()
                .flat_map(|e| vec![Response::Str(e.name), Response::Dbl(e.score)])
                .collect();
            Response::Arr(elems).encode()
        }
        _ => err_unknown("unknown command"),
    }
}

fn err_arg() -> Vec<u8> {
    Response::Err {
        code: proto::ERR_ARG,
        msg: "wrong number of arguments".into(),
    }
    .encode()
}

fn err_arg_msg(msg: &str) -> Vec<u8> {
    Response::Err {
        code: proto::ERR_ARG,
        msg: msg.into(),
    }
    .encode()
}

fn err_type(msg: &str) -> Vec<u8> {
    Response::Err {
        code: proto::ERR_TYPE,
        msg: msg.into(),
    }
    .encode()
}

fn err_unknown(msg: &str) -> Vec<u8> {
    Response::Err {
        code: proto::ERR_UNKNOWN,
        msg: msg.into(),
    }
    .encode()
}
