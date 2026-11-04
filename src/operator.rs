use clap::{Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub scopes: BTreeSet<String>,
    pub read: bool,
    pub write: bool,
}
#[derive(Clone, Copy, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    #[default]
    Turso,
    Surrealdb,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub backend: Backend,
    pub scopes: BTreeSet<String>,
    pub grants: Vec<Grant>,
}
#[derive(Subcommand)]
pub enum ScopeCommand {
    Add { id: String },
    List,
}
#[derive(Subcommand)]
pub enum GrantCommand {
    Add {
        name: String,
        #[arg(long, required = true)]
        scope: Vec<String>,
        #[arg(long)]
        read: bool,
        #[arg(long)]
        write: bool,
    },
    Rotate {
        name: String,
    },
    Revoke {
        name: String,
    },
    List,
}
pub fn default_home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/tuck")
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_:./".contains(c))
}
pub fn private(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "Cannot read local store. Check --home and permissions.".to_string())?;
    if metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(
            "Store files must belong to you, exclude group/other access, and not be symlinks."
                .into(),
        );
    }
    Ok(())
}
pub fn lock(home: &Path, name: &str) -> Result<File, String> {
    private(home)?;
    let path = home.join(name);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|_| "Cannot open store lock. Check permissions.".to_string())?;
    private(&path)?;
    file.try_lock().map_err(|_| {
        "Store is already in use. Stop the other Tuck process and retry.".to_string()
    })?;
    Ok(file)
}
pub fn load(home: &Path) -> Result<Config, String> {
    private(home)?;
    private(&home.join("config.json"))?;
    let config: Config = serde_json::from_slice(
        &fs::read(home.join("config.json")).map_err(|_| "Cannot read config.json.".to_string())?,
    )
    .map_err(|_| {
        "Invalid config.json. Restore a valid configuration before starting.".to_string()
    })?;
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    if config.scopes.iter().any(|s| !identifier(s))
        || config.grants.iter().any(|g| {
            !identifier(&g.id)
                || !identifier(&g.name)
                || !ids.insert(&g.id)
                || !names.insert(&g.name)
                || g.scopes.is_empty()
                || !g.scopes.is_subset(&config.scopes)
                || (!g.read && !g.write)
                || g.token_hash.len() != 64
                || !g.token_hash.chars().all(|c| c.is_ascii_hexdigit())
        })
    {
        return Err(
            "Invalid grants or scopes in config.json. Restore a valid configuration.".into(),
        );
    }
    Ok(config)
}
fn persist(home: &Path, config: &Config) -> Result<(), String> {
    let path = home.join(format!("config-{}.tmp", uuid::Uuid::new_v4()));
    let outcome = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(&serde_json::to_vec_pretty(config)?)?;
        file.sync_all()?;
        fs::rename(&path, home.join("config.json"))?;
        File::open(home)?.sync_all()
    })();
    let _ = fs::remove_file(path);
    outcome.map_err(|_: std::io::Error| {
        "Cannot commit configuration. Check available space and permissions.".into()
    })
}
pub async fn init(
    home: &Path,
    backend: Backend,
    initialize_storage: impl std::future::Future<Output = Result<(), String>>,
) -> Result<(), String> {
    if !home.exists() {
        fs::create_dir_all(home).map_err(|_| "Cannot create --home directory.".to_string())?;
        fs::set_permissions(home, fs::Permissions::from_mode(0o700))
            .map_err(|_| "Cannot make --home private.".to_string())?;
    }
    let _lock = lock(home, "store.lock")?;
    if fs::read_dir(home)
        .map_err(|_| "Cannot read store directory.")?
        .any(|entry| entry.map_or(true, |entry| entry.file_name() != "store.lock"))
    {
        return Err("Store already exists. Choose another --home or use serve.".into());
    }
    initialize_storage.await?;
    persist(
        home,
        &Config {
            backend,
            ..Config::default()
        },
    )?;
    println!("Initialized private Tuck store.");
    Ok(())
}
pub fn scope(home: &Path, action: ScopeCommand) -> Result<(), String> {
    let _lock = lock(home, "config.lock")?;
    let mut config = load(home)?;
    match action {
        ScopeCommand::Add { id } => {
            if !identifier(&id) {
                return Err(
                    "Scope must contain 1–128 ASCII letters, digits, or -_:./ characters.".into(),
                );
            }
            config.scopes.insert(id);
            persist(home, &config)
        }
        ScopeCommand::List => {
            for scope in config.scopes {
                println!("{scope}");
            }
            Ok(())
        }
    }
}
pub fn grant(home: &Path, action: GrantCommand) -> Result<(), String> {
    let _lock = lock(home, "config.lock")?;
    let mut config = load(home)?;
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let mut issued = false;
    match action {
        GrantCommand::Add {
            name,
            scope,
            read,
            write,
        } => {
            let scopes = scope.into_iter().collect::<BTreeSet<_>>();
            if !identifier(&name)
                || config.grants.iter().any(|g| g.name == name)
                || !scopes.is_subset(&config.scopes)
                || (!read && !write)
            {
                return Err(
                    "Choose a new grant name, existing scopes, and --read and/or --write.".into(),
                );
            }
            config.grants.push(Grant {
                id: uuid::Uuid::new_v4().to_string(),
                name,
                token_hash: hash(token.as_bytes()),
                scopes,
                read,
                write,
            });
            issued = true;
        }
        GrantCommand::Rotate { name } => {
            let grant = config
                .grants
                .iter_mut()
                .find(|g| g.name == name)
                .ok_or("Grant does not exist.")?;
            grant.token_hash = hash(token.as_bytes());
            issued = true;
        }
        GrantCommand::Revoke { name } => {
            if !config.grants.iter().any(|g| g.name == name) {
                return Err("Grant does not exist.".into());
            }
            config.grants.retain(|g| g.name != name);
        }
        GrantCommand::List => {
            for grant in config.grants {
                println!(
                    "{} read={} write={} scopes={}",
                    grant.name,
                    grant.read,
                    grant.write,
                    grant.scopes.into_iter().collect::<Vec<_>>().join(",")
                );
            }
            return Ok(());
        }
    }
    persist(home, &config)?;
    if issued {
        println!("{token}");
    }
    eprintln!("Restart Tuck to apply credential changes.");
    Ok(())
}
