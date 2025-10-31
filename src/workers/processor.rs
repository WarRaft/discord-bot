use crate::error::BotError;
use async_trait::async_trait;
use dashmap::{DashMap, mapref::entry::Entry};
use once_cell::sync::OnceCell;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;

static POOLS: OnceCell<DashMap<&'static str, Arc<Pool>>> = OnceCell::new();
fn pools() -> &'static DashMap<&'static str, Arc<Pool>> {
    POOLS.get_or_init(|| DashMap::new())
}

struct Pool {
    notify: Arc<Notify>,
    workers: AtomicUsize,
    max_workers: AtomicUsize,
}

pub enum TaskStatus {
    DidWork,
    Idle,
}

#[async_trait]
pub trait TaskProcessor: Send + Sync + 'static {
    const POOL: &'static str;
    const DEFAULT_MAX_WORKERS: usize = 1;

    async fn process_queue_item() -> Result<bool, BotError>;

    fn on_error(e: &BotError) {
        eprintln!("[{}][ERROR]: {:?}", Self::POOL, e);
    }
    async fn process_next() -> TaskStatus {
        match Self::process_queue_item().await {
            Ok(true) => TaskStatus::DidWork,
            Ok(false) => TaskStatus::Idle,
            Err(e) => {
                Self::on_error(&e);
                TaskStatus::Idle
            }
        }
    }
}

struct WorkerSlot {
    pool: Arc<Pool>,
}
impl Drop for WorkerSlot {
    fn drop(&mut self) {
        self.pool.workers.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn notify_workers<P: TaskProcessor>() {
    let pool = match pools().entry(P::POOL) {
        Entry::Occupied(o) => o.get().clone(),
        Entry::Vacant(v) => {
            let p = Arc::new(Pool {
                notify: Arc::new(Notify::new()),
                workers: AtomicUsize::new(0),
                max_workers: AtomicUsize::new(P::DEFAULT_MAX_WORKERS.max(1)),
            });
            v.insert(p.clone());
            p
        }
    };

    {
        let limit = pool.max_workers.load(Ordering::SeqCst).max(1);
        loop {
            let cur = pool.workers.load(Ordering::SeqCst);
            if cur >= limit {
                break;
            }
            if pool
                .workers
                .compare_exchange(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                // Переносим Arc, чтобы жить в таске
                let pool_for_task = pool.clone();
                let n = pool.notify.clone();
                tokio::spawn(async move {
                    let _slot = WorkerSlot {
                        pool: pool_for_task,
                    };
                    worker_loop::<P>(n).await;
                });
                break;
            }
        }
    }
    pool.notify.notify_one();
}

#[allow(dead_code)]
pub fn set_max_workers<P: TaskProcessor>(new_limit: usize) {
    if let Some(pool) = pools().get(P::POOL) {
        pool.max_workers.store(new_limit.max(1), Ordering::SeqCst);
    }
}

async fn worker_loop<P: TaskProcessor>(notify: Arc<Notify>) {
    loop {
        let notified = notify.notified();
        match P::process_next().await {
            TaskStatus::DidWork => continue,
            TaskStatus::Idle => notified.await,
        }
    }
}
