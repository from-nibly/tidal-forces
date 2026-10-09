use super::*;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};
#[derive(Default)]
struct MemoryVault {
    values: RefCell<HashMap<String, Vec<u8>>>,
    locked: Cell<bool>,
    lose_write_response: Cell<bool>,
    wrong_read: Cell<bool>,
    block_commit: RefCell<Option<std::path::PathBuf>>,
}
impl Vault for MemoryVault {
    async fn read(&self, key: &str) -> Result<Option<Vec<u8>>> {
        ensure!(!self.locked.get(), "locked test vault");
        if let Some(root) = self.block_commit.borrow_mut().take() {
            fs::remove_file(root.join("credentials.json"))?;
            fs::create_dir(root.join("credentials.json"))?;
        }
        if self.wrong_read.get() {
            return Ok(Some(b"wrong".to_vec()));
        }
        Ok(self.values.borrow().get(key).cloned())
    }
    async fn write(&self, key: &str, bytes: &[u8]) -> Result<()> {
        ensure!(!self.locked.get(), "locked test vault");
        self.values.borrow_mut().insert(key.into(), bytes.to_vec());
        ensure!(!self.lose_write_response.get(), "response lost after write");
        Ok(())
    }
    async fn delete(&self, key: &str) -> Result<()> {
        ensure!(!self.locked.get(), "locked test vault");
        self.values.borrow_mut().remove(key);
        Ok(())
    }
}
fn session(token: &str) -> Session {
    Session {
        access_token: token.into(),
        refresh_token: "synthetic-refresh".into(),
        user_id: 7,
        country: "US".into(),
        ..Session::default()
    }
}
fn legacy(root: &Path) {
    store::save_bytes(
        root,
        "session.json",
        &serde_json::to_vec(&session("old")).unwrap(),
    )
    .unwrap();
}
#[tokio::test]
async fn migration_verifies_commits_and_only_then_removes_the_legacy_copy() {
    let root = tempfile::tempdir().unwrap();
    legacy(root.path());
    let vault = MemoryVault::default();
    save_at(root.path(), &session("old"), &vault, true)
        .await
        .unwrap();
    assert_eq!(
        load_at(root.path(), &vault)
            .await
            .unwrap()
            .unwrap()
            .access_token,
        "old"
    );
    assert!(!root.path().join("session.json").exists());
    assert_eq!(
        Metadata::read(root.path()).unwrap().storage,
        Storage::Keyring
    );
    assert_eq!(vault.values.borrow().len(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(root.path().join("credentials.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let metadata = fs::read_to_string(root.path().join("credentials.json")).unwrap();
    assert!(!metadata.contains("access_token") && !metadata.contains("synthetic-refresh"));
}
#[tokio::test]
async fn locked_write_readback_and_commit_failures_never_remove_the_only_legacy_copy() {
    for failure in 0..4 {
        let root = tempfile::tempdir().unwrap();
        legacy(root.path());
        let before = fs::read(root.path().join("session.json")).unwrap();
        let vault = MemoryVault::default();
        match failure {
            0 => vault.locked.set(true),
            1 => vault.lose_write_response.set(true),
            2 => vault.wrong_read.set(true),
            _ => *vault.block_commit.borrow_mut() = Some(root.path().into()),
        }
        assert!(
            save_at(root.path(), &session("new"), &vault, true)
                .await
                .is_err()
        );
        assert_eq!(fs::read(root.path().join("session.json")).unwrap(), before);
        if failure < 3 {
            assert_eq!(
                Metadata::read(root.path()).unwrap().storage,
                Storage::Legacy
            );
        }
    }
}
#[tokio::test]
async fn interrupted_migration_cleanup_preserves_legacy_and_removes_journaled_orphans() {
    let root = tempfile::tempdir().unwrap();
    legacy(root.path());
    let vault = MemoryVault::default();
    vault.lose_write_response.set(true);
    assert!(
        save_at(root.path(), &session("new"), &vault, true)
            .await
            .is_err()
    );
    assert_eq!(vault.values.borrow().len(), 1);
    cleanup(root.path(), &vault).await.unwrap();
    assert!(vault.values.borrow().is_empty());
    assert_eq!(
        load_at(root.path(), &vault)
            .await
            .unwrap()
            .unwrap()
            .access_token,
        "old"
    );
}
#[tokio::test]
async fn refresh_is_copy_on_write_and_never_falls_back_to_plaintext() {
    let root = tempfile::tempdir().unwrap();
    legacy(root.path());
    let vault = MemoryVault::default();
    save_at(root.path(), &session("old"), &vault, true)
        .await
        .unwrap();
    let previous = Metadata::read(root.path()).unwrap().entry.unwrap();
    vault.lose_write_response.set(true);
    assert!(
        save_at(root.path(), &session("new"), &vault, false)
            .await
            .is_err()
    );
    assert_eq!(
        Metadata::read(root.path()).unwrap().entry.as_deref(),
        Some(previous.as_str())
    );
    assert_eq!(
        load_at(root.path(), &vault)
            .await
            .unwrap()
            .unwrap()
            .access_token,
        "old"
    );
    assert!(!root.path().join("session.json").exists());
    vault.lose_write_response.set(false);
    save_at(root.path(), &session("new"), &vault, false)
        .await
        .unwrap();
    assert_eq!(
        load_at(root.path(), &vault)
            .await
            .unwrap()
            .unwrap()
            .access_token,
        "new"
    );
    assert_eq!(vault.values.borrow().len(), 1);
    legacy(root.path());
    vault.locked.set(true);
    assert!(load_at(root.path(), &vault).await.is_err());
}
#[tokio::test]
async fn locked_signout_persists_intent_and_retains_cleanup_references() {
    let root = tempfile::tempdir().unwrap();
    legacy(root.path());
    let vault = MemoryVault::default();
    save_at(root.path(), &session("old"), &vault, true)
        .await
        .unwrap();
    vault.locked.set(true);
    assert!(clear_at(root.path(), &vault).await.is_err());
    assert!(load_at(root.path(), &vault).await.unwrap().is_none());
    assert!(Metadata::read(root.path()).unwrap().signed_out);
    vault.locked.set(false);
    cleanup(root.path(), &vault).await.unwrap();
    assert!(vault.values.borrow().is_empty());
    assert_eq!(
        Metadata::read(root.path()).unwrap().storage,
        Storage::Keyring
    );
    save_at(root.path(), &session("later-login"), &vault, false)
        .await
        .unwrap();
    assert!(!root.path().join("session.json").exists());
}
#[tokio::test]
async fn profiles_share_a_vault_without_reading_or_deleting_each_others_sign_in() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let vault = MemoryVault::default();
    let mut other = session("other-profile");
    other.user_id = 8;
    save_at(first.path(), &session("first-profile"), &vault, true)
        .await
        .unwrap();
    save_at(second.path(), &other, &vault, true).await.unwrap();
    clear_at(first.path(), &vault).await.unwrap();
    assert_eq!(
        load_at(second.path(), &vault)
            .await
            .unwrap()
            .unwrap()
            .user_id,
        8
    );
    assert_eq!(
        load_at(second.path(), &vault)
            .await
            .unwrap()
            .unwrap()
            .access_token,
        "other-profile"
    );
    assert_eq!(vault.values.borrow().len(), 1);
}

#[tokio::test]
async fn corrupt_metadata_and_changed_vault_contents_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    legacy(root.path());
    let vault = MemoryVault::default();
    fs::write(root.path().join("credentials.json"), b"invalid").unwrap();
    assert!(load_at(root.path(), &vault).await.is_err());
    assert!(
        save_at(root.path(), &session("new"), &vault, false)
            .await
            .is_err()
    );
    assert!(vault.values.borrow().is_empty());
    fs::remove_file(root.path().join("credentials.json")).unwrap();
    save_at(root.path(), &session("old"), &vault, true)
        .await
        .unwrap();
    let key = Metadata::read(root.path()).unwrap().entry.unwrap();
    vault
        .values
        .borrow_mut()
        .insert(key, serde_json::to_vec(&session("tampered")).unwrap());
    legacy(root.path());
    assert!(load_at(root.path(), &vault).await.is_err());
    assert!(cleanup(root.path(), &vault).await.is_err());
    assert!(root.path().join("session.json").exists());
}
#[tokio::test]
async fn cleanup_does_not_delete_legacy_credentials_changed_by_another_client() {
    let root = tempfile::tempdir().unwrap();
    legacy(root.path());
    let vault = MemoryVault::default();
    save_at(root.path(), &session("old"), &vault, true)
        .await
        .unwrap();
    let mut meta = Metadata::read(root.path()).unwrap();
    meta.legacy_digest = Some(digest(&serde_json::to_vec(&session("old")).unwrap()));
    meta.save(root.path()).unwrap();
    store::save_bytes(
        root.path(),
        "session.json",
        &serde_json::to_vec(&session("different-client")).unwrap(),
    )
    .unwrap();
    assert!(cleanup(root.path(), &vault).await.is_err());
    assert_eq!(
        decode(&fs::read(root.path().join("session.json")).unwrap())
            .unwrap()
            .access_token,
        "different-client"
    );
}
