use anyhow::{Result, ensure};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub id: String,
    pub parent: Option<String>,
    pub name: String,
}
impl Folder {
    pub fn root() -> Self {
        Self {
            id: "root".into(),
            parent: None,
            name: "Library root".into(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            valid_id(&self.id) && self.name.len() <= 512,
            "Invalid folder"
        );
        ensure!(
            if self.id == "root" {
                self.parent.is_none()
            } else {
                self.parent
                    .as_ref()
                    .is_some_and(|p| valid_id(p) && p != &self.id)
            },
            "Invalid folder parent"
        );
        Ok(())
    }
}
#[derive(Clone)]
pub enum Change {
    Create {
        parent: Folder,
        name: String,
    },
    Rename {
        folder: Folder,
        name: String,
    },
    MovePlaylist {
        id: String,
        title: String,
        source: String,
        destination: Folder,
    },
}
impl Change {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Create { parent, name } => {
                parent.validate()?;
                ensure!(
                    parent.id == "root",
                    "Nested folder creation is not supported by the verified service contract"
                );
                ensure!(valid_name(name), "Enter a folder name of 1–100 characters");
            }
            Self::Rename { folder, name } => {
                folder.validate()?;
                ensure!(
                    folder.id != "root" && valid_name(name) && name.trim() != folder.name,
                    "Choose a different folder name"
                );
            }
            Self::MovePlaylist {
                id,
                title,
                source,
                destination,
            } => {
                destination.validate()?;
                ensure!(
                    valid_id(id)
                        && id != "root"
                        && title.len() <= 512
                        && valid_id(source)
                        && source != &destination.id,
                    "Choose a different destination folder"
                );
            }
        }
        Ok(())
    }
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
pub fn valid_name(name: &str) -> bool {
    if name.len() > 400 {
        return false;
    }
    let name = name.trim();
    !name.is_empty() && name.chars().count() <= 100 && !name.chars().any(char::is_control)
}
pub struct Outcome {
    pub created: Option<Folder>,
    pub verified: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_changes_are_typed_and_cannot_move_folders_or_rename_root() {
        assert!(
            Change::Rename {
                folder: Folder::root(),
                name: "new".into()
            }
            .validate()
            .is_err()
        );
        for id in ["trn:folder:fixture", "../fixture", "root"] {
            assert!(
                Change::MovePlaylist {
                    id: id.into(),
                    title: "Fixture".into(),
                    source: "root".into(),
                    destination: Folder {
                        id: "folder-a".into(),
                        parent: Some("root".into()),
                        name: "A".into()
                    }
                }
                .validate()
                .is_err()
            );
        }
        assert!(!valid_name(" \n "));
        assert!(!valid_name(&"x".repeat(101)));
    }
}
