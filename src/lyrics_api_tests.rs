use super::playlist_tests::{mock, step};
use super::*;
use serde_json::json;

#[tokio::test]
async fn lyrics_use_authorized_track_read_and_fail_locally() {
    let mut request = step(
        "GET",
        "/v1/tracks/9/lyrics",
        json!({"trackId":9,"lyrics":"Synthetic line","subtitles":"[00:01.250]Synthetic line","lyricsProvider":"Fixture"}),
    );
    request.contains = vec!["authorization: Bearer test-only", "countryCode=US"];
    let (api, server) = mock(vec![request]);
    let lyrics = crate::lyrics::fetch(api.lyrics_request(7, 9).unwrap(), 9)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lyrics.lines[0].at, Duration::from_millis(1250));
    server.join().unwrap();
    for status in [204, 404, 401, 403, 429, 500] {
        let mut request = step("GET", "/v1/tracks/9/lyrics", json!({}));
        request.status = status;
        let (api, server) = mock(vec![request]);
        let result = crate::lyrics::fetch(api.lyrics_request(7, 9).unwrap(), 9).await;
        if matches!(status, 204 | 404) {
            assert!(result.unwrap().is_none());
        } else {
            assert!(result.is_err());
        }
        server.join().unwrap();
    }
}

#[test]
fn background_lyrics_cannot_refresh_or_cross_accounts() {
    let (mut api, server) = mock(vec![]);
    assert!(api.lyrics_request(8, 9).is_err());
    assert!(api.lyrics_request(7, 0).is_err());
    api.prohibit_refresh = true;
    api.session.as_mut().unwrap().expires_at = 0;
    assert!(api.lyrics_request(7, 9).is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn lyrics_do_not_follow_redirects_or_read_oversized_bodies() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    for redirect in [true, false] {
        let (mut api, unused) = mock(vec![]);
        unused.join().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        api.base = format!("http://{}", listener.local_addr().unwrap());
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let location = format!(
            "http://{}/must-not-be-contacted",
            target.local_addr().unwrap()
        );
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = [0; 8192];
            assert!(socket.read(&mut request).unwrap() > 0);
            if redirect {
                write!(socket, "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            } else {
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    crate::lyrics::MAX_BYTES + 1
                )
                .unwrap();
            }
        });
        assert!(
            crate::lyrics::fetch(api.lyrics_request(7, 9).unwrap(), 9)
                .await
                .is_err()
        );
        server.join().unwrap();
        assert!(
            matches!(target.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }
}
