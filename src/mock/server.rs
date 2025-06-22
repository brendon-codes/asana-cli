use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::error::{Error, Result};
use crate::mock::routes;
use crate::mock::storage::MockStorage;

#[derive(Debug)]
pub struct MockServerHandle {
    pub base_url: String,
    pub local_addr: SocketAddr,
    data_dir: PathBuf,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<Result<()>>,
}

impl MockServerHandle {
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.task
            .await
            .map_err(|error| Error::Unexpected(error.into()))?
    }
}

pub async fn serve_until_ctrl_c(bind: SocketAddr, data_dir: PathBuf) -> Result<()> {
    let listener = TcpListener::bind(bind).await.map_err(|error| {
        Error::Command(format!("failed to bind mock server on {bind}: {error}"))
    })?;
    let local_addr = listener.local_addr().map_err(|error| {
        Error::Command(format!("failed to read mock server local address: {error}"))
    })?;
    let storage = MockStorage::reset_new(&data_dir)?;
    println!("http://{local_addr}/api/1.0");
    serve(listener, data_dir, storage, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}

pub async fn spawn(bind: SocketAddr, data_dir: PathBuf) -> Result<MockServerHandle> {
    let listener = TcpListener::bind(bind).await.map_err(|error| {
        Error::Command(format!("failed to bind mock server on {bind}: {error}"))
    })?;
    let local_addr = listener.local_addr().map_err(|error| {
        Error::Command(format!("failed to read mock server local address: {error}"))
    })?;
    let storage = MockStorage::reset_new(&data_dir)?;
    let base_url = format!("http://{local_addr}/api/1.0");
    let (sender, receiver) = oneshot::channel();
    let task_data_dir = data_dir.clone();
    let task = tokio::spawn(async move {
        serve(listener, task_data_dir, storage, async {
            let _ = receiver.await;
        })
        .await
    });

    Ok(MockServerHandle {
        base_url,
        local_addr,
        data_dir,
        shutdown: Some(sender),
        task,
    })
}

async fn serve(
    listener: TcpListener,
    data_dir: PathBuf,
    storage: MockStorage,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let app = routes::router(storage);
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|error| Error::Command(format!("mock server failed: {error}")));

    MockStorage::reset_new(&data_dir)?;

    result
}
