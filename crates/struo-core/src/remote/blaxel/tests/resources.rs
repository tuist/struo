use std::time::Duration;

use super::*;

#[tokio::test]
async fn output_restoration_releases_capture_for_a_staged_competitor() {
    let limits = crate::RemoteIoLimits {
        max_archive_bytes: 16 * 1024,
        max_staging_bytes: 128 * 1024,
        max_capture_bytes: 64 * 1024,
        max_capture_staging_bytes: 64 * 1024,
        ..crate::RemoteIoLimits::default()
    };
    let resources = test_server::single_remote_quota()
        .with_remote_concurrency(2)
        .with_remote_io_limits(limits);
    let first = resources
        .acquire(Executor::Blaxel, &ResourceRequest::default())
        .await;
    let second = resources
        .acquire(Executor::Blaxel, &ResourceRequest::default())
        .await;
    let stdout = (0..40 * 1024)
        .map(|n| u8::try_from(n % 256).unwrap())
        .collect::<Vec<_>>();
    let stderr = (0..8 * 1024)
        .map(|n| u8::try_from(255 - n % 256).unwrap())
        .collect::<Vec<_>>();
    let mut replies = vec![Response::json("CREATE"), Response::empty(200)];
    replies.extend(upload_responses());
    replies.extend([
        completed(0),
        completed(0),
        bytes(&stdout),
        bytes(&stderr),
        completed(0),
        Response::tar_file("reports/result", b"passed"),
        Response::empty(200),
        Response::empty(404),
    ]);
    let (url, server, hold) = server::spawn_held(replies, 6).await;
    let workspace = workspace();
    let mut execute = Box::pin(run(url, workspace.path(), Some(first), None));
    test_server::assert_ready(async {
        tokio::select! {
            result = &mut execute => panic!("execution finished before command: {result:?}"),
            _ = hold.arrived => {}
        }
    })
    .await;
    let staging = second
        .acquire_staging(limits.max_staging_bytes)
        .await
        .unwrap();
    let capture = second.capture_budget();
    test_server::assert_pending(capture.reserve_upfront()).await;
    hold.release.send(()).unwrap();
    let competitor = async {
        capture.reserve_upfront().await.unwrap();
        drop(staging);
    };
    let (result, ()) = test_server::assert_ready(async { tokio::join!(execute, competitor) }).await;
    let result = result.unwrap();
    assert_eq!(result.exit_code, 0);
    let cache = CacheProvider::open_local(workspace.path().join("cache"));
    for (digest, expected) in [
        (result.stdout.unwrap(), stdout),
        (result.stderr.unwrap(), stderr),
    ] {
        assert_eq!(
            cache
                .read_blob_limited(&digest, limits.max_capture_bytes, false)
                .await
                .unwrap()
                .0,
            expected
        );
    }
    assert_eq!(
        std::fs::read(workspace.path().join("reports/result")).unwrap(),
        b"passed"
    );
    test_server::assert_ready(server).await.unwrap();
    assert!(records(workspace.path()).is_empty());
}

#[tokio::test]
async fn progressing_binary_download_can_exceed_sixty_seconds() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (next, mut chunks) = tokio::sync::mpsc::channel(1);
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![];
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut buffer = [0; 1024];
            let n = stream.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buffer[..n]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n\x00")
            .await
            .unwrap();
        for byte in 1..5_u8 {
            chunks.recv().await.unwrap();
            stream.write_all(&[byte]).await.unwrap();
        }
    });
    let output = tempfile::NamedTempFile::new().unwrap();
    let destination = output.path().to_owned();
    let download = tokio::spawn(async move {
        client(url)
            .download(&sandbox(), "/tmp/out", &destination, None, 1)
            .await
    });
    for expected in 1..5 {
        test_server::assert_ready(async {
            while std::fs::metadata(output.path()).unwrap().len() < expected {
                assert!(
                    !download.is_finished(),
                    "download stopped before the next chunk"
                );
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await;
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(20)).await;
        tokio::time::resume();
        next.send(()).await.unwrap();
    }
    test_server::assert_ready(download).await.unwrap().unwrap();
    test_server::assert_ready(server).await.unwrap();
    assert_eq!(std::fs::read(output.path()).unwrap(), [0, 1, 2, 3, 4]);
}

#[tokio::test]
async fn multipart_acknowledgement_uses_its_own_deadline() {
    let input = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(input.path(), b"payload").unwrap();
    let (url, server, hold) = server::spawn_held(upload_responses(), 1).await;
    let client = client(url);
    let sandbox = sandbox();
    let mut upload = Box::pin(client.upload(&sandbox, input.path(), 7, "/tmp/in"));
    test_server::assert_ready(async {
        tokio::select! {
            result = &mut upload => panic!("upload finished before acknowledgement: {result:?}"),
            _ = hold.arrived => {}
        }
    })
    .await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(61)).await;
    tokio::time::resume();
    hold.release.send(()).unwrap();
    test_server::assert_ready(upload).await.unwrap();
    test_server::assert_ready(server).await.unwrap();
}
