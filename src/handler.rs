use crate::proto::{self, Response};
use crate::store::Store;

pub async fn dispatch(store: Store, args: Vec<String>) -> Vec<u8> {
    if args.is_empty() {
        return err_unknown("empty command");
    }

    let cmd = args[0].to_lowercase();
    let arity = args.len();

    match cmd.as_str() {
        // ── Existing commands ──
        "keys" => {
            if arity != 1 { return err_arg(); }
            let keys = store.keys().await;
            let elems: Vec<Response> = keys.into_iter().map(Response::Str).collect();
            Response::Arr(elems).encode()
        }
        "get" => {
            if arity != 2 { return err_arg(); }
            let (val, wrong_type) = store.get_str(&args[1]).await;
            if wrong_type { return err_type("expect string type"); }
            match val {
                Some(v) => Response::Str(v).encode(),
                None => Response::Nil.encode(),
            }
        }
        "set" => {
            if arity != 3 { return err_arg(); }
            store.set_str(args[1].clone(), args[2].clone()).await;
            Response::Nil.encode()
        }
        "del" => {
            if arity != 2 { return err_arg(); }
            let ok = store.del(&args[1]).await;
            Response::Int(if ok { 1 } else { 0 }).encode()
        }
        "pexpire" => {
            if arity != 3 { return err_arg(); }
            let ms: i64 = match args[2].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect int64") };
            let ok = store.expire(&args[1], ms).await;
            Response::Int(if ok { 1 } else { 0 }).encode()
        }
        "pttl" => {
            if arity != 2 { return err_arg(); }
            let ttl = store.ttl_ms(&args[1]).await;
            Response::Int(ttl).encode()
        }

        // ── ZSet ──
        "zadd" => {
            if arity != 4 { return err_arg(); }
            let score: f64 = match args[2].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect fp number") };
            let (added, wrong_type) = store.z_add(args[1].clone(), args[3].clone(), score).await;
            if wrong_type { return err_type("expect zset"); }
            Response::Int(if added { 1 } else { 0 }).encode()
        }
        "zrem" => {
            if arity != 3 { return err_arg(); }
            let (removed, wrong_type) = store.z_rem(&args[1], &args[2]).await;
            if wrong_type { return err_type("expect zset"); }
            Response::Int(if removed { 1 } else { 0 }).encode()
        }
        "zscore" => {
            if arity != 3 { return err_arg(); }
            let (score, wrong_type) = store.z_score(&args[1], &args[2]).await;
            if wrong_type { return err_type("expect zset"); }
            match score {
                Some(v) => Response::Dbl(v).encode(),
                None => Response::Nil.encode(),
            }
        }
        "zquery" => {
            if arity != 6 { return err_arg(); }
            let score: f64 = match args[2].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect fp number") };
            let offset: i64 = match args[4].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect int") };
            let limit: i64 = match args[5].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect int") };
            let (entries, wrong_type) = store.z_query(&args[1], score, &args[3], offset, limit).await;
            if wrong_type { return err_type("expect zset"); }
            let elems: Vec<Response> = entries
                .into_iter()
                .flat_map(|e| vec![Response::Str(e.name), Response::Dbl(e.score)])
                .collect();
            Response::Arr(elems).encode()
        }

        // ── List (queue primitives) ──
        "lpush" => {
            if arity != 3 { return err_arg(); }
            store.lpush(args[1].clone(), args[2].clone()).await;
            Response::Int(1).encode()
        }
        "rpush" => {
            if arity != 3 { return err_arg(); }
            store.lpush(args[1].clone(), args[2].clone()).await;
            Response::Int(1).encode()
        }
        "lpop" => {
            if arity != 2 { return err_arg(); }
            match store.lpop(&args[1]).await {
                Some(v) => Response::Str(v).encode(),
                None => Response::Nil.encode(),
            }
        }
        "rpop" => {
            if arity != 2 { return err_arg(); }
            match store.rpop(&args[1]).await {
                Some(v) => Response::Str(v).encode(),
                None => Response::Nil.encode(),
            }
        }
        "lrange" => {
            if arity != 4 { return err_arg(); }
            let start: i64 = match args[2].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect int") };
            let stop: i64 = match args[3].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect int") };
            let vals = store.lrange(&args[1], start, stop).await;
            let elems: Vec<Response> = vals.into_iter().map(Response::Str).collect();
            Response::Arr(elems).encode()
        }
        "lindex" => {
            if arity != 3 { return err_arg(); }
            let idx: i64 = match args[2].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect int") };
            match store.lindex(&args[1], idx).await {
                Some(v) => Response::Str(v).encode(),
                None => Response::Nil.encode(),
            }
        }
        "llen" => {
            if arity != 2 { return err_arg(); }
            let len = store.llen(&args[1]).await;
            Response::Int(len).encode()
        }
        "lrem" => {
            if arity != 4 { return err_arg(); }
            let count: i64 = match args[2].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect int") };
            let removed = store.lrem(&args[1], count, &args[3]).await;
            Response::Int(removed).encode()
        }

        // ── Bitfield operations ──
        "setbit" => {
            if arity != 3 { return err_arg(); }
            let bit: u32 = match args[2].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect uint") };
            store.set_bit(&args[1], bit, true).await;
            Response::Int(1).encode()
        }
        "clearbit" => {
            if arity != 3 { return err_arg(); }
            let bit: u32 = match args[2].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect uint") };
            store.set_bit(&args[1], bit, false).await;
            Response::Int(1).encode()
        }
        "getbit" => {
            if arity != 3 { return err_arg(); }
            let bit: u32 = match args[2].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect uint") };
            let on = store.get_bit(&args[1], bit).await;
            Response::Int(if on { 1 } else { 0 }).encode()
        }
        "bitcount" => {
            if arity != 2 { return err_arg(); }
            let count = store.bitcount(&args[1]).await;
            Response::Int(count).encode()
        }
        "bfget" => {
            if arity != 4 { return err_arg(); }
            let offset: u32 = match args[2].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect uint") };
            let width: u32 = match args[3].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect uint") };
            let val = store.bitfield_get(&args[1], offset, width).await;
            Response::Int(val as i64).encode()
        }
        "bfset" => {
            if arity != 5 { return err_arg(); }
            let offset: u32 = match args[2].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect uint") };
            let width: u32 = match args[3].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect uint") };
            let val: u64 = match args[4].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect uint") };
            store.bitfield_set(&args[1], offset, width, val).await;
            Response::Int(1).encode()
        }

        // ── DevOps: Job queue ──
        "job submit" => {
            // JOB SUBMIT <id> <name> <target> <command> [args...]
            if arity < 5 { return err_arg_msg("JOB SUBMIT <id> <name> <target> <command> [args...]"); }
            let job_id = &args[1];
            let job_name = &args[2];
            let target = &args[3];
            let command = &args[4];
            let job_args: Vec<String> = if arity > 5 { args[5..].to_vec() } else { vec![] };
            // Format: id|name|target|command|arg1,arg2,...
            let args_str = job_args.join(",");
            let payload = format!("{}|{}|{}|{}|{}", job_id, job_name, target, command, args_str);
            store.lpush("devops:queue".to_string(), payload.clone()).await;
            store.set_str(format!("devops:job:{}", job_id), payload).await;
            store.set_str(format!("devops:job:{}:status", job_id), "queued".to_string()).await;
            Response::Str(job_id.clone()).encode()
        }
        "job next" => {
            // Pop next job from queue
            match store.rpop("devops:queue").await {
                Some(payload) => {
                    let parts: Vec<&str> = payload.splitn(5, '|').collect();
                    if parts.len() < 5 {
                        return err_unknown("malformed job payload");
                    }
                    let job_id = parts[0].to_string();
                    store.set_str(format!("devops:job:{}:status", job_id), "running".to_string()).await;
                    let elems: Vec<Response> = vec![
                        Response::Str(parts[0].to_string()),
                        Response::Str(parts[1].to_string()),
                        Response::Str(parts[2].to_string()),
                        Response::Str(parts[3].to_string()),
                        Response::Str(parts[4].to_string()),
                    ];
                    Response::Arr(elems).encode()
                }
                None => Response::Nil.encode(),
            }
        }
        "job status" => {
            if arity != 2 { return err_arg(); }
            match store.get_str(&format!("devops:job:{}:status", args[1])).await.0 {
                Some(status) => Response::Str(status).encode(),
                None => Response::Nil.encode(),
            }
        }
        "job result" => {
            // JOB RESULT <id> <exit_code> <duration_ms>
            if arity != 4 { return err_arg(); }
            let exit_code: i64 = match args[2].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect int") };
            let duration_ms: i64 = match args[3].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect int") };
            let status = if exit_code == 0 { "passed" } else { "failed" };
            store.set_str(format!("devops:job:{}:status", args[1]), status.to_string()).await;
            let result = format!("exit_code={},duration_ms={}", exit_code, duration_ms);
            store.set_str(format!("devops:job:{}:result", args[1]), result).await;
            // Record to results stream (as a list entry)
            let entry = format!("{}|{}|{}", args[1], status, duration_ms);
            store.lpush("devops:results".to_string(), entry).await;
            Response::Str(status.to_string()).encode()
        }
        "job log" => {
            // JOB LOG <id> <line>
            if arity < 3 { return err_arg(); }
            let line = args[2..].join(" ");
            let key = format!("devops:job:{}:log", args[1]);
            // Append to list
            let mut vals = store.lrange(&key, 0, -1).await;
            vals.push(line);
            store.del(&key).await;
            for v in vals {
                store.lpush(key.clone(), v).await;
            }
            Response::Int(1).encode()
        }
        "job list" => {
            // JOB LIST [status_filter]
            let status_filter = if arity >= 2 { Some(args[1].as_str()) } else { None };
            let keys = store.keys().await;
            let mut jobs = Vec::new();
            for key in &keys {
                if let Some(id) = key.strip_prefix("devops:job:") {
                    if id.ends_with(":status") || id.ends_with(":result") || id.ends_with(":log") {
                        continue;
                    }
                    if let Some(status) = store.get_str(&format!("devops:job:{}:status", id)).await.0 {
                        if status_filter.map(|f| f == status).unwrap_or(true) {
                            jobs.push(format!("{}:{}", id, status));
                        }
                    }
                }
            }
            let elems: Vec<Response> = jobs.into_iter().map(Response::Str).collect();
            Response::Arr(elems).encode()
        }

        // ── DevOps: Metrics ──
        "metric record" => {
            // METRIC RECORD <name> <value> [labels...]
            if arity < 3 { return err_arg(); }
            let name = &args[1];
            let value: f64 = match args[2].parse() { Ok(v) => v, Err(_) => return err_arg_msg("expect float") };
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis();
            let labels: Vec<String> = if arity > 3 { args[3..].to_vec() } else { vec![] };
            let entry = format!("{}|{}|{}|{}", ts, name, value, labels.join(","));
            store.lpush("devops:metrics".to_string(), entry.clone()).await;
            // Also store in a ZSet for time-series queries (score = timestamp)
            store.z_add("devops:metrics:ts".to_string(), entry, ts as f64).await;
            Response::Int(1).encode()
        }
        "metric query" => {
            // METRIC QUERY <name> [limit]
            if arity < 2 { return err_arg(); }
            let name = &args[1];
            let limit: i64 = if arity >= 3 { args[2].parse().unwrap_or(100) } else { 100 };
            let vals = store.lrange("devops:metrics", 0, limit - 1).await;
            let filtered: Vec<Response> = vals
                .into_iter()
                .filter(|v| v.contains(&format!("|{}|", name)))
                .map(Response::Str)
                .collect();
            Response::Arr(filtered).encode()
        }
        "metric summary" => {
            // METRIC SUMMARY — aggregate stats
            let vals = store.lrange("devops:metrics", 0, -1).await;
            let total = vals.len();
            let job_keys: Vec<String> = store.keys().await.into_iter().filter(|k| k.starts_with("devops:job:") && !k.contains(':')).collect();
            let total_jobs = job_keys.len();

            let mut passed = 0;
            let mut failed = 0;
            let mut queued = 0;
            for key in &job_keys {
                if let Some(id) = key.strip_prefix("devops:job:") {
                    match store.get_str(&format!("devops:job:{}:status", id)).await.0.as_deref() {
                        Some("passed") => passed += 1,
                        Some("failed") => failed += 1,
                        Some("queued") | Some("running") => queued += 1,
                        _ => {}
                    }
                }
            }
            let queue_len = store.llen("devops:queue").await;

            let summary = format!(
                "metrics={},jobs={},passed={},failed={},queued={},queue_depth={}",
                total, total_jobs, passed, failed, queued, queue_len
            );
            Response::Str(summary).encode()
        }

        // ── DevOps: Sandbox ──
        "sandbox status" => {
            // List active sandboxes
            let keys = store.keys().await;
            let sandboxes: Vec<Response> = keys
                .into_iter()
                .filter(|k| k.starts_with("devops:sandbox:"))
                .filter_map(|k| k.strip_prefix("devops:sandbox:").map(|s| s.to_string()))
                .map(Response::Str)
                .collect();
            Response::Arr(sandboxes).encode()
        }
        "sandbox register" => {
            // SANDBOX REGISTER <id> <target_type> <addr>
            if arity != 4 { return err_arg(); }
            let payload = format!("{}|{}", args[2], args[3]);
            store.set_str(format!("devops:sandbox:{}", args[1]), payload).await;
            store.set_str(format!("devops:sandbox:{}:status", args[1]), "idle".to_string()).await;
            Response::Str(args[1].clone()).encode()
        }
        "sandbox claim" => {
            // SANDBOX CLAIM <id> <job_id>
            if arity != 3 { return err_arg(); }
            store.set_str(format!("devops:sandbox:{}:status", args[1]), "busy".to_string()).await;
            store.set_str(format!("devops:sandbox:{}:job", args[1]), args[2].clone()).await;
            Response::Int(1).encode()
        }
        "sandbox release" => {
            // SANDBOX RELEASE <id>
            if arity != 2 { return err_arg(); }
            store.set_str(format!("devops:sandbox:{}:status", args[1]), "idle".to_string()).await;
            store.del(&format!("devops:sandbox:{}:job", args[1])).await;
            Response::Int(1).encode()
        }

        _ => err_unknown("unknown command"),
    }
}

fn err_arg() -> Vec<u8> {
    Response::Err { code: proto::ERR_ARG, msg: "wrong number of arguments".into() }.encode()
}

fn err_arg_msg(msg: &str) -> Vec<u8> {
    Response::Err { code: proto::ERR_ARG, msg: msg.into() }.encode()
}

fn err_type(msg: &str) -> Vec<u8> {
    Response::Err { code: proto::ERR_TYPE, msg: msg.into() }.encode()
}

fn err_unknown(msg: &str) -> Vec<u8> {
    Response::Err { code: proto::ERR_UNKNOWN, msg: msg.into() }.encode()
}
