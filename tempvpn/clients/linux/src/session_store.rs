use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{error::Result, node_client::CreatedSession};

#[derive(Debug, Default, Deserialize, Serialize)]
struct SavedSessions {
    sessions: Vec<CreatedSession>,
}

pub async fn load(path: &Path) -> Result<Vec<CreatedSession>> {
    let data = match fs::read(path).await {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    Ok(serde_json::from_slice::<SavedSessions>(&data)?.sessions)
}

pub async fn upsert(path: &Path, session: CreatedSession) -> Result<()> {
    let mut sessions = load(path).await?;
    sessions.retain(|saved| saved.session_id != session.session_id);
    sessions.push(session);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(&SavedSessions { sessions })?,
    )
    .await?;
    set_private_permissions(&temporary).await?;
    fs::rename(temporary, path).await?;
    set_private_permissions(path).await?;
    Ok(())
}

#[cfg(unix)]
async fn set_private_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    Ok(())
}

#[cfg(not(unix))]
async fn set_private_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use tempfile::tempdir;

    use super::*;

    fn session(id: &str, remaining_seconds: u64) -> CreatedSession {
        CreatedSession {
            session_id: id.into(),
            node_url: Some("https://node.test".into()),
            not_after: Utc::now() + chrono::Duration::days(7),
            total_seconds: remaining_seconds,
            remaining_seconds,
            state: "paused".into(),
        }
    }

    #[tokio::test]
    async fn persists_capabilities_privately_and_updates_in_place() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("sessions.json");
        upsert(&path, session("sess_one", 60)).await.unwrap();
        upsert(&path, session("sess_one", 120)).await.unwrap();
        let saved = load(&path).await.unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].remaining_seconds, 120);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
