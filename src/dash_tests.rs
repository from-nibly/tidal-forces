use super::*;
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener},
    sync::atomic::{AtomicBool, Ordering},
};

struct FixtureServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl FixtureServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            let files: [&[u8]; 5] = [
                include_bytes!("../tests/fixtures/dash/0.mp4"),
                include_bytes!("../tests/fixtures/dash/1.mp4"),
                include_bytes!("../tests/fixtures/dash/2.mp4"),
                include_bytes!("../tests/fixtures/dash/3.mp4"),
                include_bytes!("../tests/fixtures/dash/4.mp4"),
            ];
            while !stopped.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut request = [0; 4096];
                        let count = socket.read(&mut request).unwrap_or(0);
                        let text = String::from_utf8_lossy(&request[..count]);
                        let index = text.split_whitespace().nth(1).and_then(|s| {
                            s.trim_start_matches('/')
                                .trim_end_matches(".mp4")
                                .parse::<usize>()
                                .ok()
                        });
                        if let Some(bytes) = index.and_then(|i| files.get(i)) {
                            let _ = write!(
                                socket,
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                bytes.len()
                            );
                            let _ = socket.write_all(bytes);
                        } else {
                            let _ = socket.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            address,
            stop,
            thread: Some(thread),
        }
    }
    fn manifest(&self) -> Manifest {
        let mut start = 0;
        let segments = [46080u64, 46080, 46080, 5760]
            .into_iter()
            .enumerate()
            .map(|(index, frames)| {
                let segment = Segment {
                    url: format!("http://{}/{}.mp4", self.address, index + 1),
                    start: Duration::from_secs_f64(start as f64 / 44100.),
                    duration: Duration::from_secs_f64(frames as f64 / 44100.),
                };
                start += frames;
                segment
            })
            .collect();
        Manifest {
            init: format!("http://{}/0.mp4", self.address),
            segments,
            sample_rate: 44100,
            duration: Duration::from_secs_f64(3.2),
        }
    }
}
impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lossless_segments_are_contiguous_and_seekable_with_bounded_buffering() {
    let server = FixtureServer::start();
    let mut audio = source(server.manifest()).await.unwrap();
    tokio::task::spawn_blocking(move || {
        assert_eq!(audio.channels(), 1);
        assert_eq!(audio.sample_rate(), 44100);
        let decoded: Vec<_> = audio.by_ref().collect();
        assert_eq!(
            decoded.len(),
            141120,
            "No missing or duplicate frames at segment boundaries"
        );
        audio.try_seek(Duration::from_millis(1500)).unwrap();
        let after_seek: Vec<_> = audio.by_ref().take(30000).collect();
        assert_eq!(after_seek, decoded[66150..96150]);
        audio.try_seek(Duration::ZERO).unwrap();
        assert_eq!(
            audio.by_ref().take(1000).collect::<Vec<_>>(),
            decoded[..1000]
        );
        assert!(audio.cache.state.lock().unwrap().chunks.len() <= 3);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unavailable_initialization_segment_is_reported() {
    let server = FixtureServer::start();
    let mut manifest = server.manifest();
    manifest.init = format!("http://{}/404.mp4", server.address);
    assert!(source(manifest).await.is_err());
}
