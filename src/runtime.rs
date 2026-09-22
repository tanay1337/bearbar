use std::sync::OnceLock;

use tokio::runtime::{Builder, Runtime};

use crate::error::{Error, Result};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

pub fn initialize() -> Result<()> {
    if RUNTIME.get().is_some() {
        return Ok(());
    }

    let runtime = Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("bearbar-worker")
        .enable_all()
        .build()
        .map_err(Error::Runtime)?;

    let _ = RUNTIME.set(runtime);
    Ok(())
}

pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    RUNTIME
        .get()
        .expect("runtime must be initialized before spawning work")
        .spawn(future);
}
