use std::path::{Path, PathBuf};

use serde::Deserialize;

pub struct ZulipCredentials {
    pub url: String,
    pub email: String,
    pub api_key: String,
}

// --- ~/.zulookrc TOML format ---

#[derive(Deserialize)]
struct ZulookConfig {
    api: Vec<ApiEntry>,
}

#[derive(Deserialize)]
struct ApiEntry {
    site: String,
    email: String,
    key: String,
}

// --- Credential store: lazy resolution by URL ---

pub struct CredentialStore {
    /// Entries from ~/.zulookrc (multi-instance)
    zulookrc_entries: Vec<ApiEntry>,
    /// Single entry from ~/zuliprc (legacy)
    zuliprc: Option<ZulipCredentials>,
    /// Single entry from env vars
    env: Option<ZulipCredentials>,
}

impl CredentialStore {
    /// Load all available credential sources.
    /// Fails if ~/.zulookrc exists but can't be parsed (syntax errors
    /// should be reported, not silently ignored).
    pub fn load() -> anyhow::Result<Self> {
        let zulookrc_entries = match default_zulookrc_path().filter(|p| p.exists()) {
            Some(p) => load_zulookrc(&p)?,
            None => Vec::new(),
        };

        let zuliprc = default_zuliprc_path()
            .filter(|p| p.exists())
            .and_then(|p| load_from_zuliprc(&p).ok());

        let env = load_from_env().ok();

        Ok(CredentialStore {
            zulookrc_entries,
            zuliprc,
            env,
        })
    }

    /// Resolve credentials for a given Zulip base URL (e.g. "https://rust-lang.zulipchat.com").
    pub fn resolve(&self, base_url: &str) -> anyhow::Result<ZulipCredentials> {
        let normalized = base_url.trim_end_matches('/');

        // 1. ~/.zulookrc: match by site
        for entry in &self.zulookrc_entries {
            let site = entry.site.trim_end_matches('/');
            if normalized.eq_ignore_ascii_case(site) {
                return Ok(ZulipCredentials {
                    url: entry.site.clone(),
                    email: entry.email.clone(),
                    api_key: entry.key.clone(),
                });
            }
        }

        // 2. ~/zuliprc: use unconditionally (single-instance assumption)
        if let Some(ref creds) = self.zuliprc {
            return Ok(ZulipCredentials {
                url: creds.url.clone(),
                email: creds.email.clone(),
                api_key: creds.api_key.clone(),
            });
        }

        // 3. Env vars: use unconditionally
        if let Some(ref creds) = self.env {
            return Ok(ZulipCredentials {
                url: creds.url.clone(),
                email: creds.email.clone(),
                api_key: creds.api_key.clone(),
            });
        }

        anyhow::bail!(
            "No Zulip credentials found for {base_url}. Either:\n\
             - Add an [[api]] entry in ~/.zulookrc (run `zulook setup` for details)\n\
             - Place a zuliprc file at ~/zuliprc\n\
             - Set ZULIP_URL, ZULIP_EMAIL, ZULIP_API_KEY environment variables"
        )
    }
}

// --- Loaders ---

fn load_zulookrc(path: &Path) -> anyhow::Result<Vec<ApiEntry>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Could not read {}: {e}", path.display()))?;
    let config: ZulookConfig = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Could not parse {}: {e}", path.display()))?;
    Ok(config.api)
}

/// Parse a zuliprc file (INI format with [api] section).
fn load_from_zuliprc(path: &Path) -> anyhow::Result<ZulipCredentials> {
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

fn load_from_env() -> anyhow::Result<ZulipCredentials> {
    let url = std::env::var("ZULIP_URL").map_err(|_| anyhow::anyhow!("ZULIP_URL not set"))?;
    let email = std::env::var("ZULIP_EMAIL").map_err(|_| anyhow::anyhow!("ZULIP_EMAIL not set"))?;
    let api_key =
        std::env::var("ZULIP_API_KEY").map_err(|_| anyhow::anyhow!("ZULIP_API_KEY not set"))?;
    Ok(ZulipCredentials {
        url,
        email,
        api_key,
    })
}

// --- Path helpers ---

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn default_zulookrc_path() -> Option<PathBuf> {
    home_dir().map(|home| home.join(".zulookrc"))
}

/// Default zuliprc path: ~/zuliprc (matching zulip-terminal convention).
fn default_zuliprc_path() -> Option<PathBuf> {
    home_dir().map(|home| home.join("zuliprc"))
}
