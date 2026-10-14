use super::*;
use crate::ui::tests::fixture;

fn prepare(app: &mut App) {
    app.page = Page::Collection {
        kind: "playlists".into(),
        id: "fixture-list".into(),
        title: "Old".into(),
    };
    app.playlist_page = Some(PlaylistPage {
        playlist: Playlist {
            uuid: "fixture-list".into(),
            title: "Old".into(),
            description: Some("Old description".into()),
            ..Default::default()
        },
        etag: "revision-1".into(),
        editable: true,
        rows: vec![],
        next_offset: 0,
        more: false,
    });
    app.browse_loaded();
    app.open_metadata_editor();
}

#[test]
fn metadata_confirmation_freezes_scope_and_prevents_double_dispatch() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    assert!(requests.try_recv().is_err());
    let editor = app.metadata_editor.as_mut().unwrap();
    editor.title = "New".into();
    editor.description.clear();
    let revision = app.queue.revision();
    app.save_metadata_editor();
    app.save_metadata_editor();
    assert!(
        matches!(requests.try_recv(),Ok(Request::EditPlaylistMetadata{user:7,id,etag,title,description,..}) if id=="fixture-list" && etag=="revision-1" && title=="New" && description.is_empty())
    );
    assert!(requests.try_recv().is_err());
    assert!(app.metadata_editor.as_ref().unwrap().busy);
    assert!(!app.browse_cache.ready);
    assert_eq!(app.queue.revision(), revision);
}

#[test]
fn metadata_editor_requires_known_owned_current_account_scope() {
    for case in 0..4 {
        let (_, mut app, _events, mut requests) = fixture();
        prepare(&mut app);
        match case {
            0 => app.account = Some(8),
            1 => app.generation += 1,
            2 => app.connected = false,
            _ => app.metadata_editor.as_mut().unwrap().title = " ".into(),
        }
        app.save_metadata_editor();
        assert!(requests.try_recv().is_err());
    }
    let (_, mut app, _events, _requests) = fixture();
    prepare(&mut app);
    app.metadata_editor = None;
    app.playlist_page.as_mut().unwrap().playlist.description = None;
    app.open_metadata_editor();
    assert!(app.metadata_editor.is_none());
    app.playlist_page.as_mut().unwrap().playlist.description = Some("".into());
    app.playlist_page.as_mut().unwrap().editable = false;
    app.open_metadata_editor();
    assert!(app.metadata_editor.is_none());
}

#[test]
fn metadata_failures_require_refresh_and_old_account_replies_are_ignored() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    let operation = app.metadata_serial;
    app.save_metadata_editor();
    requests.try_recv().unwrap();
    app.metadata_result(
        8,
        operation,
        "fixture-list".into(),
        Err("wrong account".into()),
    );
    assert!(app.metadata_editor.as_ref().unwrap().error.is_none());
    app.metadata_result(
        7,
        operation,
        "fixture-list".into(),
        Err("Refresh before retrying".into()),
    );
    app.save_metadata_editor();
    assert!(requests.try_recv().is_err());
    assert!(app.metadata_editor.as_ref().unwrap().error.is_some());
}

#[test]
fn metadata_worker_backpressure_leaves_no_permanent_busy_editor() {
    let (_, mut app, _events, requests) = fixture();
    prepare(&mut app);
    drop(requests);
    app.save_metadata_editor();
    let editor = app.metadata_editor.as_ref().unwrap();
    assert!(!editor.busy);
    assert!(editor.error.is_some());
}
