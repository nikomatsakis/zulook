use std::path::{Path, PathBuf};

pub struct ZulipCredentials {
    pub url: String,
    pub email: String,
    pub api_key: String,
}

/// Parse a zuliprc file (INI format with [api] section).
pub fn load_from_zuliprc(path: &Path) -> anyhow::Result<ZulipCredentials> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Could not read {}: {e}", path.display()))?;

    let mut email = None;
    let mut key = None;
    let mut site = None;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') || line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "email" => email = Some(v.trim().to_string()),
                "key" => key = Some(v.trim().to_string()),
                "site" => site = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }

    Ok(ZulipCredentials {
        url: site.ok_or_else(|| anyhow::anyhow!("No 'site' found in {}", path.display()))?,
        email: email.ok_or_else(|| anyhow::anyhow!("No 'email' found in {}", path.display()))?,
        api_key: key.ok_or_else(|| anyhow::anyhow!("No 'key' found in {}", path.display()))?,
    })
}

/// Get the user's home directory.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Default zuliprc path: ~/zuliprc (matching zulip-terminal convention).
pub fn default_zuliprc_path() -> Option<PathBuf> {
    home_dir().map(|home| home.join("zuliprc"))
}

fn load_from_env() -> anyhow::Result<ZulipCredentials> {
    let url = std::env::var("ZULIP_URL").map_err(|_| anyhow::anyhow!("ZULIP_URL not set"))?;
    let email = std::env::var("ZULIP_EMAIL").map_err(|_| anyhow::anyhow!("ZULIP_EMAIL not set"))?;
    let api_key = std::env::var("ZULIP_API_KEY").map_err(|_| anyhow::anyhow!("ZULIP_API_KEY not set"))?;
    Ok(ZulipCredentials { url, email, api_key })
}

/// Load credentials: ~/zuliprc > env vars.
pub fn load_credentials() -> anyhow::Result<ZulipCredentials> {
    if let Some(path) = default_zuliprc_path() {
        if path.exists() {
            if let Ok(creds) = load_from_zuliprc(&path) {
                return Ok(creds);
            }
        }
    }

    load_from_env().map_err(|_| {
        anyhow::anyhow!(
            "No Zulip credentials found. Either:\n\
             - Place a zuliprc file at ~/zuliprc (run `zulip-mcp setup` for instructions)\n\
             - Set ZULIP_URL, ZULIP_EMAIL, ZULIP_API_KEY environment variables"
        )
    })
}
