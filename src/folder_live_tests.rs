use super::*;
use serde_json::json;

struct Journal {
    root: std::path::PathBuf,
    user: u64,
    prefix: String,
    folders: Vec<Folder>,
    playlist: Option<(String, String)>,
    cascade: Option<bool>,
}
impl Journal {
    fn save(&self, stage: &str) -> Result<()> {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut events = options.open(self.root.join("events.jsonl"))?;
        writeln!(events, "{}", serde_json::json!({"stage":stage}))?;
        events.sync_all()?;
        let folders: Vec<_> = self
            .folders
            .iter()
            .map(|f| json!({"id":f.id,"parent":f.parent,"name":f.name}))
            .collect();
        crate::store::save_bytes(
            &self.root,
            "receipt.json",
            &serde_json::to_vec(
                &json!({"user":self.user,"prefix":self.prefix,"folders":folders,"playlist":self.playlist,"cascadeDeletesOwnedPlaylist":self.cascade,"stage":stage}),
            )?,
        )
    }
    async fn create(&mut self, api: &mut Api, parent: Folder, suffix: &str) -> Result<Folder> {
        let name = format!("{} {suffix}", self.prefix);
        self.save(&format!("creating-{suffix}"))?;
        let outcome = api
            .manage_folder(
                self.user,
                &Change::Create { parent, name },
                &AtomicBool::new(false),
            )
            .await?;
        let folder = outcome.created.context("Missing created folder identity")?;
        self.folders.push(folder.clone());
        self.save("folder-created")?;
        anyhow::ensure!(outcome.verified, "Created folder was not verified");
        Ok(folder)
    }
    async fn move_playlist(&self, api: &mut Api, source: &str, destination: Folder) -> Result<()> {
        let (id, title) = self.playlist.as_ref().context("No test playlist")?;
        anyhow::ensure!(
            api.manage_folder(
                self.user,
                &Change::MovePlaylist {
                    id: id.clone(),
                    title: title.clone(),
                    source: source.into(),
                    destination
                },
                &AtomicBool::new(false)
            )
            .await?
            .verified,
            "Move was not verified"
        );
        Ok(())
    }
    async fn remove_folder(&self, api: &mut Api, folder: &Folder) -> Result<()> {
        anyhow::ensure!(
            self.folders.iter().any(|f| f.id == folder.id),
            "Cleanup scope mismatch"
        );
        api.folder_scope(self.user, &AtomicBool::new(false))?;
        api.request(
            Method::PUT,
            "v2",
            "my-collection/playlists/folders/remove",
            &[("trns", &format!("trn:folder:{}", folder.id))],
            &[],
            None,
        )
        .await?;
        let entries = api
            .folder_snapshot(
                self.user,
                folder.parent.as_deref().context("Missing parent")?,
                &AtomicBool::new(false),
            )
            .await?;
        anyhow::ensure!(
            !entries
                .iter()
                .any(|e| matches!(e,LibraryEntry::Folder{id,..} if id==&folder.id)),
            "Folder removal not verified"
        );
        Ok(())
    }
    async fn cleanup(&self, api: &mut Api) -> Result<()> {
        if let Some((id, title)) = &self.playlist {
            match api.playlist_page(id, 0, None).await {
                Ok(page) => {
                    anyhow::ensure!(
                        page.editable
                            && &page.playlist.title == title
                            && page.playlist.number_of_tracks == 0
                            && page.playlist.number_of_videos == 0,
                        "Temporary playlist changed; manual inspection required"
                    );
                    api.delete_playlist(self.user, id, &page.etag, &AtomicBool::new(false))
                        .await?;
                }
                Err(error) => anyhow::ensure!(
                    error
                        .downcast_ref::<RequestNotFound>()
                        .is_some_and(|e| e.path == format!("playlists/{id}")),
                    "Could not inspect temporary playlist for cleanup"
                ),
            }
        }
        for folder in self.folders.iter().rev() {
            let entries = api
                .folder_snapshot(
                    self.user,
                    folder.parent.as_deref().context("Missing parent")?,
                    &AtomicBool::new(false),
                )
                .await?;
            if !entries
                .iter()
                .any(|e| matches!(e,LibraryEntry::Folder{id,..} if id==&folder.id))
            {
                continue;
            }
            let expected_name = has_folder(&entries, &folder.id, &folder.name)
                || (self.folders.get(1).is_some_and(|f| f.id == folder.id)
                    && has_folder(&entries, &folder.id, &format!("{} renamed", self.prefix)));
            anyhow::ensure!(
                expected_name,
                "Temporary folder changed; manual inspection required"
            );
            anyhow::ensure!(
                api.folder_snapshot(self.user, &folder.id, &AtomicBool::new(false))
                    .await?
                    .is_empty(),
                "Temporary folder is not empty; refusing cascading cleanup"
            );
            self.remove_folder(api, folder).await?;
        }
        Ok(())
    }
}

#[tokio::test]
#[ignore = "Creates/renames/moves/deletes only temporary test folders and playlist; separate explicit operator opt-in required"]
async fn live_temporary_folder_round_trip() {
    assert_eq!(
        std::env::var("TIDAL_TEST_FOLDERS").as_deref(),
        Ok("1"),
        "Explicit opt-in required"
    );
    let mut api = Api::new().expect("Could not initialize client");
    api.prohibit_refresh = true;
    api.session = api.credentials.load().await.ok().flatten();
    let session = api
        .session
        .as_ref()
        .expect("A signed-in session is required");
    assert!(
        session.user_id != 0 && session.expires_at > store::now() + 240,
        "A fresh session is required; no credential renewal was attempted"
    );
    let user = session.user_id;
    let identity = api
        .get("sessions", &[])
        .await
        .unwrap_or_else(|_| panic!("Identity check failed"));
    assert!(
        identity["userId"].as_u64() == Some(user),
        "Account identity changed"
    );
    let root = tempfile::Builder::new()
        .prefix("tidal-folder-live.")
        .tempdir()
        .unwrap()
        .keep();
    let mut journal = Journal {
        root,
        user,
        prefix: format!("TF temporary verification {:016x}", rand::random::<u64>()),
        folders: vec![],
        playlist: None,
        cascade: None,
    };
    journal.save("starting").unwrap();
    // Only the private receipt path is printed, never account data or response bodies.
    println!(
        "Private temporary-object receipt: {}",
        journal.root.display()
    );
    let result=tokio::time::timeout(Duration::from_secs(90),async {
        let _a=journal.create(&mut api,Folder::root(),"A").await?;
        let mut b=journal.create(&mut api,Folder::root(),"B").await?;
        let name=format!("{} renamed",journal.prefix);journal.save("renaming")?;
        anyhow::ensure!(api.manage_folder(user,&Change::Rename {folder:b.clone(),name:name.clone()},&AtomicBool::new(false)).await?.verified,"Rename not verified");
        b.name=name;journal.folders[1]=b.clone();journal.save("renamed")?;
        let title=format!("{} playlist",journal.prefix);journal.save("creating-playlist")?;
        let playlist=api.create_playlist(&title,"Temporary folder verification; no audio tracks; removed after the test.").await?;
        journal.playlist=Some((playlist.uuid,title));journal.save("playlist-created")?;
        journal.save("moving-in")?;journal.move_playlist(&mut api,"root",b.clone()).await?;
        journal.save("moving-out")?;journal.move_playlist(&mut api,&b.id,Folder::root()).await?;
        journal.save("moving-in-again")?;journal.move_playlist(&mut api,"root",b.clone()).await?;
        let entries=api.folder_snapshot(user,&b.id,&AtomicBool::new(false)).await?;
        let (id,title)=journal.playlist.as_ref().unwrap();
        anyhow::ensure!(entries.len()==1 && matches!(&entries[0],LibraryEntry::Playlist(p) if &p.uuid==id && &p.title==title),"Nonempty removal test contains an unexpected item");
        journal.save("testing-cascading-removal")?;
        journal.remove_folder(&mut api,&b).await?;
        journal.cascade=Some(match api.get(&format!("playlists/{id}"),&[]).await {
            Ok(_)=>false,
            Err(error)=>{anyhow::ensure!(error.downcast_ref::<RequestNotFound>().is_some_and(|e|e.path==format!("playlists/{id}")),"Could not verify playlist after folder removal");true}
        });
        journal.save("round-trip-finished")?;
        Ok::<(),anyhow::Error>(())
    }).await;
    let succeeded = matches!(result, Ok(Ok(())));
    let recorded = journal
        .save(if succeeded {
            "cleaning-up"
        } else {
            "operation-failed-cleaning-known-objects"
        })
        .is_ok();
    let cleaned = matches!(
        tokio::time::timeout(Duration::from_secs(90), journal.cleanup(&mut api)).await,
        Ok(Ok(()))
    );
    let final_recorded = journal
        .save(if cleaned {
            if succeeded {
                "complete-cleaned"
            } else {
                "failed-known-objects-cleaned-inspect-uncertain-creations"
            }
        } else {
            "cleanup-incomplete-manual-inspection-required"
        })
        .is_ok();
    assert!(
        cleaned,
        "Cleanup incomplete; inspect the private receipt. No automatic retry was attempted."
    );
    assert!(
        succeeded,
        "Folder round trip failed; known objects cleaned. Inspect receipt for any uncertain creation before retrying."
    );
    assert!(
        recorded && final_recorded,
        "Could not persist the cleanup receipt"
    );
    assert!(!api.credentials_dirty, "Probe must not modify credentials");
}
