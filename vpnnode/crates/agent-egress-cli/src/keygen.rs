use tokio::{
    io::AsyncWriteExt,
    process::{ChildStdin, Command},
};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Keypair {
    pub private_key: String,
    pub public_key: String,
}

pub async fn generate(wg_command: &str) -> Result<Keypair> {
    let output = Command::new(wg_command)
        .arg("genkey")
        .output()
        .await
        .map_err(Error::Io)?;
    if !output.status.success() {
        return Err(Error::CommandFailed {
            program: format!("{wg_command} genkey"),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    let private_key = String::from_utf8(output.stdout)?.trim().to_string();
    let public_key = public_key(wg_command, &private_key).await?;
    Ok(Keypair {
        private_key,
        public_key,
    })
}

async fn public_key(wg_command: &str, private_key: &str) -> Result<String> {
    let mut child = Command::new(wg_command)
        .arg("pubkey")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(Error::Io)?;

    write_private_key(child.stdin.take(), private_key).await?;
    let output = child.wait_with_output().await?;
    if !output.status.success() {
        return Err(Error::CommandFailed {
            program: format!("{wg_command} pubkey"),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

async fn write_private_key(stdin: Option<ChildStdin>, private_key: &str) -> Result<()> {
    let mut stdin = stdin.ok_or(Error::MissingPubkeyStdin)?;
    stdin.write_all(private_key.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    Ok(())
}
