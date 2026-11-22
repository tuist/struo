use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::Sleep;

/// Avoids read-ahead across wire frames so partial messages cannot hide in the decoder.
pub(super) struct Transport<S> {
    inner: S,
    header: [u8; 14],
    filled: usize,
    needed: usize,
    payload: u64,
    fragmented: bool,
    timeout: Duration,
    read_timer: Option<Pin<Box<Sleep>>>,
    write_timer: Option<Pin<Box<Sleep>>>,
}

impl<S> Transport<S> {
    pub fn new(inner: S, timeout: Duration) -> Self {
        Self {
            inner,
            header: [0; 14],
            filled: 0,
            needed: 2,
            payload: 0,
            fragmented: false,
            timeout,
            read_timer: None,
            write_timer: None,
        }
    }

    fn frame_done(&mut self) {
        let opcode = self.header[0] & 15;
        if opcode < 8 {
            self.fragmented = self.header[0] & 128 == 0;
        }
        self.filled = 0;
        self.needed = 2;
        if !self.fragmented {
            self.read_timer = None;
        }
    }
}

fn expired(timer: &mut Option<Pin<Box<Sleep>>>, cx: &mut Context<'_>) -> bool {
    timer
        .as_mut()
        .is_some_and(|timer| timer.as_mut().poll(cx).is_ready())
}

fn stalled() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "WebSocket transfer stalled")
}

impl<S: AsyncRead + Unpin> AsyncRead for Transport<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if expired(&mut this.read_timer, cx) {
            return Poll::Ready(Err(stalled()));
        }
        let limit = if this.payload > 0 {
            usize::try_from(this.payload).unwrap_or(usize::MAX)
        } else {
            this.needed - this.filled
        }
        .min(buf.remaining());
        let mut part = ReadBuf::new(&mut buf.initialize_unfilled()[..limit]);
        match Pin::new(&mut this.inner).poll_read(cx, &mut part) {
            Poll::Ready(Ok(())) => {
                let bytes = part.filled();
                let count = bytes.len();
                if count > 0 {
                    this.read_timer = Some(Box::pin(tokio::time::sleep(this.timeout)));
                    if this.payload > 0 {
                        this.payload -= count as u64;
                        if this.payload == 0 {
                            this.frame_done();
                        }
                    } else {
                        this.header[this.filled..this.filled + count].copy_from_slice(bytes);
                        this.filled += count;
                        if this.filled == 2 {
                            this.needed =
                                2 + match this.header[1] & 127 {
                                    126 => 2,
                                    127 => 8,
                                    _ => 0,
                                } + if this.header[1] & 128 != 0 { 4 } else { 0 };
                        }
                        if this.filled == this.needed {
                            this.payload = match this.header[1] & 127 {
                                126 => u64::from(u16::from_be_bytes(
                                    this.header[2..4].try_into().unwrap(),
                                )),
                                127 => u64::from_be_bytes(this.header[2..10].try_into().unwrap()),
                                n => u64::from(n),
                            };
                            if this.payload == 0 {
                                this.frame_done();
                            }
                        }
                    }
                }
                buf.advance(count);
                Poll::Ready(Ok(()))
            }
            Poll::Pending => {
                if expired(&mut this.read_timer, cx) {
                    Poll::Ready(Err(stalled()))
                } else {
                    Poll::Pending
                }
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
        }
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Transport<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        this.write_timer
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(this.timeout)));
        if expired(&mut this.write_timer, cx) {
            return Poll::Ready(Err(stalled()));
        }
        let result = Pin::new(&mut this.inner).poll_write(cx, bytes);
        if result.is_ready() {
            this.write_timer = None;
        } else {
            let _ = expired(&mut this.write_timer, cx);
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        this.write_timer
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(this.timeout)));
        if expired(&mut this.write_timer, cx) {
            return Poll::Ready(Err(stalled()));
        }
        let result = Pin::new(&mut this.inner).poll_flush(cx);
        if result.is_ready() {
            this.write_timer = None;
        } else {
            let _ = expired(&mut this.write_timer, cx);
        }
        result
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.as_mut().poll_flush(cx) {
            Poll::Ready(Ok(())) => Pin::new(&mut self.inner).poll_shutdown(cx),
            result => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{SinkExt, StreamExt};
    use tokio::io::AsyncWriteExt;
    use tokio_tungstenite::{
        tungstenite::{protocol::Role, Message},
        WebSocketStream,
    };

    #[tokio::test]
    async fn partial_frames_stall_but_complete_messages_allow_silence() {
        for bytes in [
            vec![0x82],
            vec![0x82, 4, 1],
            vec![0x02, 1, 1],
            vec![0x82, 126, 0],
            vec![0x82, 127, 0, 0],
            vec![0x82, 2, 3, 0, 0x82, 4, 1],
            vec![0x02, 1, 1, 0x89, 0],
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (mut peer, _) = listener.accept().await.unwrap();
            let mut ws = WebSocketStream::from_raw_socket(
                Transport::new(client, Duration::from_millis(25)),
                Role::Client,
                None,
            )
            .await;
            peer.write_all(&bytes).await.unwrap();
            if bytes.starts_with(&[0x82, 2, 3, 0]) {
                assert!(ws.next().await.unwrap().is_ok());
            }
            let result = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let result = ws.next().await.unwrap();
                    if result.is_err() {
                        break result;
                    }
                }
            })
            .await
            .unwrap();
            assert!(
                matches!(result, Err(tokio_tungstenite::tungstenite::Error::Io(error)) if error.kind() == io::ErrorKind::TimedOut)
            );
        }
        let (client, mut peer) = tokio::io::duplex(64);
        let mut ws = WebSocketStream::from_raw_socket(
            Transport::new(client, Duration::from_millis(25)),
            Role::Client,
            None,
        )
        .await;
        peer.write_all(&[0x82, 2, 3, 7]).await.unwrap();
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Binary(vec![3, 7].into())
        );
        assert!(tokio::time::timeout(Duration::from_millis(75), ws.next())
            .await
            .is_err());
        peer.write_all(&[0x82, 2, 3, 9]).await.unwrap();
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Binary(vec![3, 9].into())
        );
    }

    #[tokio::test]
    async fn blocked_control_write_stalls() {
        let (client, _unread_peer) = tokio::io::duplex(1);
        let mut ws = WebSocketStream::from_raw_socket(
            Transport::new(client, Duration::from_millis(25)),
            Role::Client,
            None,
        )
        .await;
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            ws.send(Message::Pong(vec![1, 2, 3].into())),
        )
        .await
        .unwrap();
        assert!(
            matches!(result, Err(tokio_tungstenite::tungstenite::Error::Io(error)) if error.kind() == io::ErrorKind::TimedOut)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn progressing_message_can_exceed_the_stall_timeout() {
        let (client, mut peer) = tokio::io::duplex(64);
        let mut ws = WebSocketStream::from_raw_socket(
            Transport::new(client, Duration::from_secs(2)),
            Role::Client,
            None,
        )
        .await;
        let writer = async {
            for bytes in [&[0x02, 2][..], &[1][..], &[2][..], &[0x80, 1][..], &[3][..]] {
                peer.write_all(bytes).await.unwrap();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        };
        let reader = async {
            assert_eq!(
                ws.next().await.unwrap().unwrap(),
                Message::Binary(vec![1, 2, 3].into())
            );
        };
        tokio::join!(writer, reader);
    }
}
