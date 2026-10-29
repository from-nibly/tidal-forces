use super::*;
use std::{
    io::{Cursor, Read, Write},
    net::TcpListener,
    time::Instant,
};

fn stream() -> (Stream, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).unwrap() > 0);
        let bytes = include_bytes!("../tests/fixtures/tone.flac");
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )
        .unwrap();
        socket.write_all(bytes).unwrap();
    });
    (
        Stream {
            source: StreamSource::Direct(format!("http://{address}/synthetic")),
            quality: "LOSSLESS".into(),
            codec: "FLAC".into(),
            sample_rate: Some(44100),
            bit_depth: Some(16),
        },
        server,
    )
}
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Requires documented private null-ALSA configuration; never run against real output"]
async fn output_recovery_on_isolated_null_alsa() {
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::env::var("TIDAL_TEST_ISOLATED_NULL_OUTPUT").as_deref(),
        Ok("1")
    );
    let config = std::path::PathBuf::from(std::env::var_os("ALSA_CONFIG_PATH").unwrap());
    assert!(
        config.starts_with("/tmp")
            && config
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("tidal-v030-null-")
    );
    assert_eq!(
        std::fs::metadata(config.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        "pcm.!default { type tidal_test_missing_plugin }\n"
    );
    fn receive(rx: &mpsc::Receiver<Event>, matches: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let event = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            if matches(&event) {
                return event;
            }
        }
    }
    let (tx, rx) = mpsc::channel();
    let player = Player::new(Events::fixture(tx));
    let id = player.reserve();
    let (source, server) = stream();
    player.load(id, source, 0).await.unwrap();
    server.join().unwrap();
    let Event::OutputLost {
        generation, epoch, ..
    } = receive(&rx, |e| matches!(e, Event::OutputLost { .. }))
    else {
        unreachable!()
    };
    assert_eq!(generation, id);
    std::fs::write(&config, "pcm.!default { type null }\n").unwrap();
    std::fs::File::open(&config)
        .unwrap()
        .set_modified(std::time::SystemTime::now() + Duration::from_secs(2))
        .unwrap();
    // A healthy endpoint alone, or a new track generation, must not resume output.
    let id = player.reserve();
    let (source, server) = stream();
    player.load(id, source, 0).await.unwrap();
    server.join().unwrap();
    let Event::OutputLost {
        generation,
        epoch: again,
        ..
    } = receive(&rx, |e| matches!(e, Event::OutputLost { .. }))
    else {
        unreachable!()
    };
    assert_eq!((generation, again), (id, epoch));
    player.retry_output(epoch);
    let id = player.reserve();
    let (source, server) = stream();
    player.load(id, source, 0).await.unwrap();
    server.join().unwrap();
    let event = receive(&rx, |e| {
        matches!(e, Event::Playing { .. } | Event::OutputLost { .. })
    });
    assert!(matches!(event, Event::Playing { generation, .. } if generation == id));
    // Exercise the real owner loop's fault path without an audible/hardware test.
    player.tx.send(Command::FailOutput).unwrap();
    let Event::OutputLost {
        generation,
        epoch: newer,
        position,
    } = receive(&rx, |e| matches!(e, Event::OutputLost { .. }))
    else {
        unreachable!()
    };
    assert_eq!((generation, position), (id, Some(0)));
    assert!(newer > epoch);
    player.retry_output(epoch); // stale acknowledgement must not reopen anything
    let id = player.reserve();
    let (source, server) = stream();
    player.load(id, source, 0).await.unwrap();
    server.join().unwrap();
    let Event::OutputLost {
        generation, epoch, ..
    } = receive(&rx, |e| matches!(e, Event::OutputLost { .. }))
    else {
        unreachable!()
    };
    assert_eq!((generation, epoch), (id, newer));
    player.retry_output(newer);
    let id = player.reserve();
    let (source, server) = stream();
    player.load(id, source, 0).await.unwrap();
    server.join().unwrap();
    assert!(
        matches!(receive(&rx, |e| matches!(e, Event::Playing { .. } | Event::OutputLost { .. })), Event::Playing { generation, .. } if generation == id)
    );
    player.stop();
}

fn key(generation: u64) -> NextKey {
    NextKey {
        user: 7,
        generation,
        revision: 1,
        current: 1,
        occurrence: 2,
        track: 2,
        quality: "LOSSLESS".into(),
        duration: 1,
    }
}
fn wait_for(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prepared_flac_joins_and_seeks_after_fixture_servers_are_closed() {
    let mut player = Player::inert();
    let (tx, rx) = mpsc::channel();
    player.tx = tx;
    let id = player.reserve();
    assert!(matches!(rx.recv().unwrap(), Command::Stop));
    let (current, server) = stream();
    player.load(id, current, 0).await.unwrap();
    server.join().unwrap();
    let Command::Load(_, pcm, _, feedback, _) = rx.recv().unwrap() else {
        panic!("Expected current PCM");
    };
    let previous = Frame::new(id, &pcm, feedback, 0);
    let request = player.begin_next(key(id)).unwrap();
    let (next, server) = stream();
    player.prepare_next(request.clone(), next).await.unwrap();
    server.join().unwrap();
    let Command::Prepare(token, next, session, feedback) = rx.recv().unwrap() else {
        panic!("Expected prepared PCM");
    };
    let slot = Slot::default();
    let prepared = NextOutput {
        staged: &player.next,
        sessions: &player.loaded,
        generation: &player.generation,
        capture: &player.visualizer,
        slot: &slot,
    }
    .arm(Some(&previous), token, next, *session, feedback)
    .unwrap();
    let mut chain = Chain::new(
        pcm,
        previous.clone(),
        slot,
        player.visualizer.clone(),
        player.generation.clone(),
    );
    let reference: Vec<_> = Decoder::new(Cursor::new(
        include_bytes!("../tests/fixtures/tone.flac").as_slice(),
    ))
    .unwrap()
    .collect();
    let channels = usize::from(chain.channels());
    let mut first = Vec::new();
    let mut second = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline);
        if first.len() == reference.len() - channels {
            wait_for(|| previous.feedback.finished());
        }
        if second.len() == reference.len() - channels {
            wait_for(|| prepared.frame.feedback.finished());
        }
        let Some(sample) = chain.next() else {
            break;
        };
        let next = player.current(request.boundary.to);
        let frame = if next { &prepared.frame } else { &previous };
        if frame.feedback.activity().load(Ordering::Acquire) {
            if next {
                second.push(sample);
            } else {
                first.push(sample);
            }
        } else {
            assert!(
                first.len() != reference.len() || !second.is_empty(),
                "No underflow between a completed source and its ready successor"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    assert_eq!(first, reference);
    assert_eq!(second, reference);
    let transition = player.reconcile_next(false).unwrap();
    assert_eq!(transition.generation, request.boundary.to);
    assert!(player.loaded.lock().unwrap().as_ref().unwrap().flac);
    drop(chain);
    let seek = player.reserve_seek(transition.generation).unwrap();
    assert!(matches!(rx.recv().unwrap(), Command::Stop));
    player.seek_buffered(seek, 0).await.unwrap();
    let Command::Load(generation, mut pcm, _, feedback, _) = rx.recv().unwrap() else {
        panic!("Expected local next-song seek");
    };
    assert_eq!(generation, seek);
    let mut samples = Vec::new();
    while samples.len() < 128 {
        assert!(Instant::now() < deadline);
        let sample = pcm.next().unwrap();
        if feedback.activity().load(Ordering::Acquire) {
            samples.push(sample);
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    assert_eq!(samples, reference[..128]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_preparation_cannot_arm_and_aac_is_not_claimed_gapless() {
    let mut player = Player::inert();
    let (tx, rx) = mpsc::channel();
    player.tx = tx;
    player.seed_buffered_fixture();
    let request = player.begin_next(key(0)).unwrap();
    let next = Stream {
        source: StreamSource::Direct("must-not-be-requested".into()),
        quality: "HIGH".into(),
        codec: "AAC".into(),
        sample_rate: None,
        bit_depth: None,
    };
    assert!(
        player
            .prepare_next(request.clone(), next)
            .await
            .unwrap_err()
            .to_string()
            .contains("priming")
    );
    player.fail_next(&request);
    assert!(player.current(0));
    while rx.try_recv().is_ok() {}
    let request = player.begin_next(key(0)).unwrap();
    let (next, server) = stream();
    player.prepare_next(request.clone(), next).await.unwrap();
    server.join().unwrap();
    let Command::Prepare(token, pcm, session, feedback) = rx.recv().unwrap() else {
        panic!("Expected preparation");
    };
    let frame = Frame::new(0, &pcm, feedback.clone(), 0);
    let replacement = player.reserve();
    assert_ne!(replacement, request.boundary.to);
    let slot = Slot::default();
    assert!(
        NextOutput {
            staged: &player.next,
            sessions: &player.loaded,
            generation: &player.generation,
            capture: &player.visualizer,
            slot: &slot
        }
        .arm(Some(&frame), token, pcm, *session, feedback)
        .is_none()
    );
    assert!(player.reconcile_next(false).is_none());
    assert!(player.current(replacement));
}
