use super::playlist_tests::{mock, step};
use super::*;
use serde_json::json;

#[tokio::test]
async fn only_confirmed_missing_catalog_tracks_are_unavailable() {
    for catalog_status in [200, 404, 403, 503] {
        let mut playback = step("GET", "/v1/tracks/9/playbackinfopostpaywall", json!({}));
        playback.status = 404;
        let mut catalog = step("GET", "/v1/tracks/9", json!({"id":9}));
        catalog.status = catalog_status;
        let (mut api, server) = mock(vec![playback, catalog]);
        let error = api.stream(9, "LOSSLESS").await.err().unwrap();
        assert_eq!(
            error.downcast_ref::<TrackUnavailable>().is_some(),
            catalog_status == 404
        );
        server.join().unwrap();
    }
}
#[tokio::test]
async fn only_playback_401_readiness_is_skippable_not_other_authorization_or_transient_errors() {
    for status in [401, 403, 429, 500, 503] {
        let mut playback = step(
            "GET",
            "/v1/tracks/9/playbackinfopostpaywall",
            json!({"status":status,"subStatus":4005}),
        );
        playback.status = status;
        let (mut api, server) = mock(vec![playback]);
        let error = api.stream(9, "LOSSLESS").await.err().unwrap();
        assert_eq!(
            error.downcast_ref::<TrackUnavailable>().copied(),
            (status == 401).then_some(TrackUnavailable::NotReady)
        );
        server.join().unwrap();
    }
}
#[tokio::test]
async fn unrelated_401_errors_never_become_readiness_skips_or_contact_real_auth() {
    for (path, code) in [
        ("/v1/tracks/9", 4005),
        ("/v1/tracks/9/playbackinfopostpaywall", 11003),
    ] {
        let mut rejected = step("GET", path, json!({"status":401,"subStatus":code}));
        rejected.status = 401;
        let (mut api, server) = mock(vec![rejected]);
        api.prohibit_refresh = true;
        let error = api
            .get(path.strip_prefix("/v1/").unwrap(), &[])
            .await
            .unwrap_err();
        assert!(error.downcast_ref::<TrackUnavailable>().is_none());
        server.join().unwrap();
    }
}
#[test]
fn normalization_metadata_has_no_defaults_or_album_to_track_substitution() {
    let manifest=STANDARD.encode(serde_json::to_vec(&json!({"encryptionType":"NONE","urls":["https://example.invalid/fixture"],"codecs":"FLAC"})).unwrap());
    let mut value = json!({"manifestMimeType":"application/vnd.tidal.bts","manifest":manifest});
    assert!(
        !parse_stream(&value)
            .unwrap()
            .normalization
            .available(crate::dsp::Normalization::Track)
    );
    value["trackReplayGain"] = json!(-8.);
    let metadata = parse_stream(&value).unwrap().normalization;
    assert!(metadata.available(crate::dsp::Normalization::Track));
    assert!(!metadata.available(crate::dsp::Normalization::Album));
    value["albumReplayGain"] = json!(1e100);
    assert!(
        !parse_stream(&value)
            .unwrap()
            .normalization
            .available(crate::dsp::Normalization::Album)
    );
    value["trackReplayGain"] = json!("-8");
    assert!(
        !parse_stream(&value)
            .unwrap()
            .normalization
            .available(crate::dsp::Normalization::Track)
    );
}
