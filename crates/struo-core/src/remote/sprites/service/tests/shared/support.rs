use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};

use struo_cas::{CacheProvider, HttpCacheAuth, HttpCacheRemote, TransferBudget};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use super::super::super::{Binding, Service};
use crate::remote::{CommandFilesystemSetup, PreparedCommand};
use crate::{RemoteIoLimits, ResourceRequest, WorkspacePath};

pub(super) struct Endpoint {
    pub url: String,
    pub requests: Arc<Mutex<Vec<String>>>,
    server: JoinHandle<()>,
    proxy: JoinHandle<()>,
}

impl Endpoint {
    async fn proxy(address: SocketAddr, server: JoinHandle<()>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let proxy = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let recorded = recorded.clone();
                tokio::spawn(async move {
                    let mut header = Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        let Ok(byte) = stream.read_u8().await else {
                            return;
                        };
                        header.push(byte);
                        assert!(header.len() < 65536);
                    }
                    let line = String::from_utf8_lossy(&header)
                        .lines()
                        .next()
                        .unwrap()
                        .to_string();
                    recorded
                        .lock()
                        .unwrap()
                        .push(line.trim_end_matches(" HTTP/1.1").to_string());
                    let Ok(mut upstream) = TcpStream::connect(address).await else {
                        return;
                    };
                    if upstream.write_all(&header).await.is_ok() {
                        let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
                    }
                });
            }
        });
        Self {
            url,
            requests,
            server,
            proxy,
        }
    }

    pub async fn cache(root: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let path = root.join("shared");
        let server = tokio::spawn(async move {
            struo_cas::server::serve(
                listener,
                path,
                "cache-only-credential".into(),
                4 * 1024 * 1024,
                TransferBudget::new(2 * 1024 * 1024, 4 * 1024 * 1024),
            )
            .await
            .unwrap();
        });
        Self::proxy(address, server).await
    }

    pub fn remote(&self) -> HttpCacheRemote {
        HttpCacheRemote::new(
            &self.url,
            None,
            HttpCacheAuth::Bearer("cache-only-credential".into()),
        )
        .unwrap()
    }

    pub fn stop(&mut self) {
        self.server.abort();
        self.proxy.abort();
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(super) struct Worker {
    pub service: Service,
    pub endpoint: Endpoint,
}

impl Worker {
    pub async fn start(root: &Path, memory_bytes: u64) -> Option<Self> {
        let prepared = root.join("prepared");
        std::fs::create_dir_all(prepared.join("data")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let path = root.join("service");
        let unsupported = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let isolation_failure = unsupported.clone();
        let server = tokio::spawn(async move {
            let result = crate::worker::serve(
                listener,
                path,
                "0123456789abcdef0123456789abcdef".into(),
                crate::worker::ServeOptions {
                    environment: "test".into(),
                    preparation_root: prepared,
                    tool_environment: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
                    tool_launcher: Vec::new(),
                    memory_bytes,
                    capacity_bytes: 4 * 1024 * 1024,
                },
            )
            .await;
            if let Err(error) = result {
                assert!(error.to_string().contains("bubblewrap"), "{error}");
                eprintln!("ISOLATION NOT EXERCISED: {error}");
                isolation_failure.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        });
        let endpoint = Endpoint::proxy(address, server).await;
        let service = Service::new(
            &Binding {
                url: endpoint.url.clone(),
                token: "0123456789abcdef0123456789abcdef".into(),
                binary: String::new(),
            },
            RemoteIoLimits::default(),
        )
        .unwrap();
        if service.ready("test").await.is_err() && endpoint.server.is_finished() {
            assert!(
                unsupported.load(std::sync::atomic::Ordering::SeqCst),
                "unexpected worker startup failure"
            );
            return None;
        }
        Some(Self { service, endpoint })
    }
}

pub(super) async fn run(
    service: &Service,
    cache: &CacheProvider,
    workspace: &Path,
    input: &str,
    output: &[&str],
    script: &str,
) {
    let inputs: Vec<WorkspacePath> = vec![input.parse().unwrap()];
    let outputs = output
        .iter()
        .map(|path| path.parse().unwrap())
        .collect::<Vec<_>>();
    let argv = vec!["/bin/sh".into(), "-c".into(), format!("set -eu; {script}")];
    let env = BTreeMap::new();
    let resources = ResourceRequest::new(1, 1024 * 1024);
    let result = service
        .execute(
            "test",
            &PreparedCommand {
                argv: &argv,
                env: &env,
                inputs: &inputs,
                outputs: &outputs,
                cwd: None,
                resources: &resources,
                redirect: crate::local::Redirect::default(),
                timeout_ms: Some(5000),
                success_exit_codes: &[0],
                lease: None,
            },
            workspace,
            cache,
            false,
            CommandFilesystemSetup::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
}
