use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use redisops::store::Store;
use redisops::handler;

fn bench_set_get(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let store = Store::new();
    
    let mut group = c.benchmark_group("kv_store");
    for size in [1, 10, 100, 1000] {
        group.bench_with_input(BenchmarkId::new("set", size), &size, |b, &size| {
            b.iter(|| {
                rt.block_on(async {
                    for i in 0..size {
                        let key = format!("key:{}", i);
                        let val = format!("value:{}", i);
                        handler::dispatch(store.clone(), vec!["SET".into(), key, val]).await;
                    }
                });
            });
        });
        
        group.bench_with_input(BenchmarkId::new("get", size), &size, |b, &size| {
            // Pre-populate
            rt.block_on(async {
                for i in 0..size {
                    let key = format!("key:{}", i);
                    let val = format!("value:{}", i);
                    handler::dispatch(store.clone(), vec!["SET".into(), key, val]).await;
                }
            });
            b.iter(|| {
                rt.block_on(async {
                    for i in 0..size {
                        let key = format!("key:{}", i);
                        handler::dispatch(store.clone(), vec!["GET".into(), key]).await;
                    }
                });
            });
        });
    }
    group.finish();
}

fn bench_btree(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    let mut group = c.benchmark_group("btree");
    for size in [100, 1000, 10000] {
        group.bench_with_input(BenchmarkId::new("insert", size), &size, |b, &size| {
            b.iter(|| {
                rt.block_on(async {
                    let store = Store::new();
                    for i in 0..size {
                        let key = format!("btree:{}", i);
                        let val = format!("val:{}", i);
                        handler::dispatch(store.clone(), vec!["SET".into(), key, val]).await;
                    }
                });
            });
        });
    }
    group.finish();
}

fn bench_list(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let store = Store::new();
    
    let mut group = c.benchmark_group("list");
    for size in [10, 100, 1000] {
        group.bench_with_input(BenchmarkId::new("lpush_rpop", size), &size, |b, &size| {
            b.iter(|| {
                rt.block_on(async {
                    for i in 0..size {
                        let val = format!("item:{}", i);
                        handler::dispatch(store.clone(), vec!["LPUSH".into(), "bench:list".into(), val.clone()]).await;
                        handler::dispatch(store.clone(), vec!["RPOP".into(), "bench:list".into()]).await;
                    }
                });
            });
        });
    }
    group.finish();
}

fn bench_concurrent(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let store = Store::new();
    
    let mut group = c.benchmark_group("concurrent");
    group.bench_function("parallel_writes_100", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut handles = vec![];
                for i in 0..100 {
                    let s = store.clone();
                    handles.push(tokio::spawn(async move {
                        let key = format!("conc:{}", i);
                        let val = format!("val:{}", i);
                        handler::dispatch(s, vec!["SET".into(), key, val]).await;
                    }));
                }
                for h in handles {
                    h.await.unwrap();
                }
            });
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_set_get,
    bench_btree,
    bench_list,
    bench_concurrent
);
criterion_main!(benches);
