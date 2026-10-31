use super::playlist_tests::{mock, step};
use super::*;
use serde_json::json;

#[tokio::test]
async fn recommendation_snapshot_uses_only_authorized_track_radio_and_classifies_failures() {
    for status in [200, 204, 404, 401, 403, 429, 500] {
        let mut request = step(
            "GET",
            "/v1/tracks/9/radio",
            json!({"items":[{"id":10,"title":"Synthetic track","duration":180}]}),
        );
        request.status = status;
        request.contains = vec![
            "authorization: Bearer test-only",
            "countryCode=US",
            "limit=100",
            "offset=0",
        ];
        let (api, server) = mock(vec![request]);
        let result = crate::recommendations::fetch(api.recommendation_request(7, 9).unwrap()).await;
        match status {
            200 => assert_eq!(result.unwrap()[0].id, 10),
            204 | 404 => assert!(result.unwrap().is_empty()),
            _ => assert!(result.is_err()),
        }
        server.join().unwrap();
    }
}

#[test]
fn recommendations_do_not_refresh_credentials_or_cross_accounts() {
    let (mut api, server) = mock(vec![]);
    assert!(api.recommendation_request(8, 9).is_err());
    assert!(api.recommendation_request(7, 0).is_err());
    api.prohibit_refresh = true;
    api.session.as_mut().unwrap().expires_at = 0;
    assert!(api.recommendation_request(7, 9).is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn recommendation_reads_refuse_redirects_and_oversized_bodies() {
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
        let location = format!("http://{}/not-contacted", target.local_addr().unwrap());
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
                    "HTTP/1.1 200 OK\r\nContent-Length: 1048577\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            }
        });
        assert!(
            crate::recommendations::fetch(api.recommendation_request(7, 9).unwrap())
                .await
                .is_err()
        );
        server.join().unwrap();
        assert!(
            matches!(target.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }
}
