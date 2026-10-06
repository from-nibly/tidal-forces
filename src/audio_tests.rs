use crate::audio::decode;

// Locally synthesized tones, not TIDAL audio. Keep the MP4 metadata at the end
// so decoding exercises the byte-length/seek path used by real AAC streams.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn decodes_and_seeks_aac_mp4_and_flac_streams() {
    for bytes in [
        include_bytes!("../tests/fixtures/tone.m4a").as_slice(),
        include_bytes!("../tests/fixtures/tone.flac").as_slice(),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            assert!(socket.read(&mut request).unwrap() > 0);
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .unwrap();
            socket.write_all(bytes).unwrap();
        });
        let reader = StreamDownload::new_http(
            format!("http://{address}/audio").parse().unwrap(),
            TempStorageProvider::new(),
            Settings::default(),
        )
        .await
        .unwrap();
        assert_eq!(reader.content_length(), Some(bytes.len() as u64));
        let mut decoder = decode(reader).await.unwrap();
        tokio::task::spawn_blocking(move || {
            assert_eq!(decoder.sample_rate(), 44100);
            assert!(decoder.next().is_some());
            decoder.try_seek(Duration::from_millis(500)).unwrap();
            assert!(decoder.take(1000).any(|sample| sample.abs() > 0.01));
        })
        .await
        .unwrap();
        server.join().unwrap();
    }
}
use rodio::Source;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};
use stream_download::{Settings, StreamDownload, storage::temp::TempStorageProvider};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streams_and_decodes_audio_from_http_without_an_external_player() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let samples = 44100u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&44100u32.to_le_bytes());
        wav.extend_from_slice(&88200u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples * 2).to_le_bytes());
        for _ in 0..samples {
            wav.extend_from_slice(&1000i16.to_le_bytes());
        }
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).unwrap();
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", wav.len()).unwrap();
        for chunk in wav.chunks(8192) {
            socket.write_all(chunk).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    let reader = StreamDownload::new_http(
        format!("http://{addr}/test.wav").parse().unwrap(),
        TempStorageProvider::new(),
        Settings::default().prefetch_bytes(4096),
    )
    .await
    .unwrap();
    let decoder = decode(reader).await.unwrap();
    assert_eq!(decoder.channels(), 1);
    assert_eq!(decoder.sample_rate(), 44100);
    let count = tokio::task::spawn_blocking(move || {
        let samples: Vec<_> = decoder.collect();
        assert!(samples.iter().all(|s| *s > 0.0));
        samples.len()
    })
    .await
    .unwrap();
    assert_eq!(count, 44100);
    server.join().unwrap();
}
