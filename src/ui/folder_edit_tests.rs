use super::*;
use crate::ui::tests::fixture;
fn prepare(app: &mut App) {
    app.folders.insert("root".into(), vec![]);
}
#[test]
fn folder_forms_are_inert_until_submission_and_dispatch_only_once() {
    let (_, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    app.new_folder();
    assert!(requests.try_recv().is_err());
    app.submit_folder_edit();
    assert!(requests.try_recv().is_err());
    let dialog = app.folder_dialog.as_mut().unwrap();
    let Change::Create { name, .. } = &mut dialog.change else {
        panic!()
    };
    *name = "Fixture".into();
    let queue = app.queue.revision();
    app.submit_folder_edit();
    app.submit_folder_edit();
    let Request::ManageFolder {
        user,
        operation,
        cancelled,
        ..
    } = requests.try_recv().unwrap()
    else {
        panic!()
    };
    assert_eq!(user, 7);
    assert!(!cancelled.load(Ordering::Acquire));
    assert!(requests.try_recv().is_err());
    app.folder_result(
        8,
        operation,
        Ok(Outcome {
            created: None,
            verified: true,
        }),
    );
    assert!(app.folder_dialog.as_ref().unwrap().busy);
    app.folder_result(
        7,
        operation + 1,
        Ok(Outcome {
            created: None,
            verified: true,
        }),
    );
    assert!(app.folder_dialog.as_ref().unwrap().busy);
    app.folder_result(
        7,
        operation,
        Ok(Outcome {
            created: None,
            verified: true,
        }),
    );
    assert!(app.folder_dialog.is_none());
    assert_eq!(app.queue.revision(), queue);
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::Folder { user: 7, .. })
    ));
}
#[test]
fn cancelling_or_unverified_folder_changes_require_fresh_folder_reads() {
    for cancel in [true, false] {
        let (_, mut app, _events, mut requests) = fixture();
        prepare(&mut app);
        app.new_folder();
        let Change::Create { name, .. } = &mut app.folder_dialog.as_mut().unwrap().change else {
            panic!()
        };
        *name = "Fixture".into();
        app.submit_folder_edit();
        let Request::ManageFolder {
            operation,
            cancelled,
            ..
        } = requests.try_recv().unwrap()
        else {
            panic!()
        };
        if cancel {
            app.cancel_folder_edit();
        } else {
            app.folder_result(
                7,
                operation,
                Ok(Outcome {
                    created: None,
                    verified: false,
                }),
            );
        }
        assert!(cancelled.load(Ordering::Acquire));
        assert!(app.folder_refresh_required);
        assert!(!app.can_edit_folders());
        assert!(requests.try_recv().is_err());
        app.cancel_folder_edit();
        app.refresh_folders();
        assert!(matches!(
            requests.try_recv(),
            Ok(Request::Folder { user: 7, .. })
        ));
        assert!(!app.can_edit_folders());
    }
}
#[test]
fn old_folder_reads_cannot_satisfy_refresh_or_cross_accounts() {
    let (_, mut app, events, mut requests) = fixture();
    app.folders.clear();
    app.request_folder("root");
    let Request::Folder { serial: old, .. } = requests.try_recv().unwrap() else {
        panic!()
    };
    app.refresh_folders();
    let Request::Folder { serial: new, .. } = requests.try_recv().unwrap() else {
        panic!()
    };
    assert_ne!(old, new);
    events
        .send(Event::Folder {
            user: 7,
            serial: old,
            id: "root".into(),
            entries: vec![],
        })
        .unwrap();
    app.events();
    assert!(!app.folders.contains_key("root"));
    events
        .send(Event::Folder {
            user: 8,
            serial: new,
            id: "root".into(),
            entries: vec![],
        })
        .unwrap();
    app.events();
    assert!(!app.folders.contains_key("root"));
    events
        .send(Event::Folder {
            user: 7,
            serial: new,
            id: "root".into(),
            entries: vec![],
        })
        .unwrap();
    app.events();
    assert!(app.folders.contains_key("root"));
}
#[test]
fn focused_folder_rows_offer_a_keyboard_context_menu() {
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    for (index, press) in [false, true, false].into_iter().enumerate() {
        let modifiers = egui::Modifiers {
            shift: press,
            ..egui::Modifiers::NONE
        };
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(900., 650.),
                )),
                modifiers,
                events: if press {
                    vec![egui::Event::Key {
                        key: egui::Key::F10,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    }]
                } else {
                    vec![]
                },
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let row = folder_button(ui, "Fixture", false);
                    row.request_focus();
                    app.folder_menu(
                        &row,
                        Folder {
                            id: "folder-a".into(),
                            parent: Some("root".into()),
                            name: "Fixture".into(),
                        },
                    );
                });
            },
        );
        if index == 2 {
            assert!(output.shapes.iter().any(
                |s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="Rename folder…")
            ));
        }
    }
    assert!(requests.try_recv().is_err());
}

#[test]
fn folder_submit_button_uses_the_guarded_controller() {
    let (ctx, mut app, _events, mut requests) = fixture();
    prepare(&mut app);
    app.new_folder();
    let Change::Create { name, .. } = &mut app.folder_dialog.as_mut().unwrap().change else {
        panic!()
    };
    *name = "Fixture".into();
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            vec2(900., 650.),
        )),
        ..Default::default()
    };
    let _ = ctx.run(input(), |ctx| app.folder_window(ctx));
    let output = ctx.run(input(), |ctx| app.folder_window(ctx));
    let target = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Create folder" => {
                Some(t.pos + t.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap();
    for pressed in [true, false] {
        let mut input = input();
        input.events = vec![
            egui::Event::PointerMoved(target),
            egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        let _ = ctx.run(input, |ctx| app.folder_window(ctx));
    }
    assert!(matches!(
        requests.try_recv(),
        Ok(Request::ManageFolder { user: 7, .. })
    ));
    assert!(requests.try_recv().is_err());
}
