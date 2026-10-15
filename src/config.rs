//! Where markoff finds its data.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use color_eyre::{
    Result,
    eyre::{WrapErr, bail},
};

/// Env var with the root directory, used when no `--dir` is given.
const DIR_ENV: &str = "MARKOFF_DIR";

/// Resolves the root directory (the one containing `todo/` and `archive/`).
///
/// Order: `--dir <path>` / `--dir=<path>`, then [`DIR_ENV`], then the config file.
/// `config` is only called if the first two are missing, so a broken config never blocks
/// `--dir`. Args, env and config are parameters so this can be tested without the real process.
pub fn root_dir(
    mut args: impl Iterator<Item = String>,
    env: Option<String>,
    config: impl FnOnce() -> Result<Option<PathBuf>>,
) -> Result<PathBuf> {
    let mut dir = None;
    while let Some(arg) = args.next() {
        if arg == "--dir" {
            let Some(value) = args.next() else {
                bail!("--dir needs a path");
            };
            dir = Some(value);
        } else if let Some(value) = arg.strip_prefix("--dir=") {
            dir = Some(value.to_string());
        } else {
            bail!("unknown argument: {arg}");
        }
    }

    if let Some(dir) = dir.or(env) {
        return Ok(PathBuf::from(dir));
    }
    match config()? {
        Some(dir) => Ok(dir),
        None => bail!(
            "no directory given, use --dir <path>, set {DIR_ENV}, \
             or set `dir` in ~/.config/markoff/config.toml"
        ),
    }
}

/// Reads the real process args, env and config file.
pub fn root_dir_from_env() -> Result<PathBuf> {
    let var = |name| std::env::var(name).ok().filter(|v: &String| !v.is_empty());
    let home = var("HOME");
    root_dir(
        std::env::args().skip(1),
        var(DIR_ENV),
        || match config_path(var("XDG_CONFIG_HOME"), home.clone()) {
            Some(path) => read_config(&path, home.as_deref()),
            None => Ok(None),
        },
    )
}

/// `$XDG_CONFIG_HOME/markoff/config.toml`, falling back to `~/.config/markoff/config.toml`.
fn config_path(xdg_config_home: Option<String>, home: Option<String>) -> Option<PathBuf> {
    let base = match xdg_config_home {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(home?).join(".config"),
    };
    Some(base.join("markoff").join("config.toml"))
}

/// The `dir` from the config file, `None` if the file or the key does not exist.
fn read_config(path: &Path, home: Option<&str>) -> Result<Option<PathBuf>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).wrap_err_with(|| format!("failed to read {}", path.display())),
    };
    let table: toml::Table = content
        .parse()
        .wrap_err_with(|| format!("invalid config {}", path.display()))?;

    let Some(value) = table.get("dir") else {
        return Ok(None);
    };
    let Some(dir) = value.as_str() else {
        bail!("`dir` in {} must be a string", path.display());
    };
    let config_dir = path.parent().unwrap_or(Path::new("."));
    resolve(dir, config_dir, home).map(Some)
}

/// Expands a leading `~/`, and resolves relative paths against the config file's directory
/// (not the current directory, so markoff behaves the same wherever it is started).
fn resolve(dir: &str, config_dir: &Path, home: Option<&str>) -> Result<PathBuf> {
    let path = match dir.strip_prefix("~/") {
        Some(rest) => {
            let Some(home) = home else {
                bail!("cannot expand `~` in {dir}, HOME is not set");
            };
            Path::new(home).join(rest)
        }
        None => PathBuf::from(dir),
    };
    Ok(config_dir.join(path))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    fn no_config() -> Result<Option<PathBuf>> {
        Ok(None)
    }

    #[test]
    fn flag_wins_over_env() {
        let dir = root_dir(args(&["--dir", "/a"]), Some("/b".into()), no_config).unwrap();
        assert_eq!(dir, PathBuf::from("/a"));
        let dir = root_dir(args(&["--dir=/a"]), Some("/b".into()), no_config).unwrap();
        assert_eq!(dir, PathBuf::from("/a"));
    }

    #[test]
    fn env_fallback() {
        let dir = root_dir(args(&[]), Some("/b".into()), no_config).unwrap();
        assert_eq!(dir, PathBuf::from("/b"));
    }

    #[test]
    fn config_fallback_and_only_read_when_needed() {
        let config = || Ok(Some(PathBuf::from("/c")));
        assert_eq!(
            root_dir(args(&[]), None, config).unwrap(),
            PathBuf::from("/c")
        );

        let broken = || bail!("broken config");
        let dir = root_dir(args(&["--dir", "/a"]), None, broken).unwrap();
        assert_eq!(dir, PathBuf::from("/a"));
    }

    #[test]
    fn errors() {
        assert!(root_dir(args(&[]), None, no_config).is_err());
        assert!(root_dir(args(&["--dir"]), None, no_config).is_err());
        assert!(root_dir(args(&["--nope"]), None, no_config).is_err());
    }

    #[test]
    fn config_path_prefers_xdg() {
        let path = config_path(Some("/xdg".into()), Some("/home/me".into()));
        assert_eq!(path, Some(PathBuf::from("/xdg/markoff/config.toml")));
        let path = config_path(None, Some("/home/me".into()));
        assert_eq!(
            path,
            Some(PathBuf::from("/home/me/.config/markoff/config.toml"))
        );
        assert_eq!(config_path(None, None), None);
    }

    #[test]
    fn read_config_cases() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let read = |content: &str| {
            fs::write(&path, content).unwrap();
            read_config(&path, Some("/home/me"))
        };

        assert_eq!(read("dir = \"/abs\"").unwrap(), Some(PathBuf::from("/abs")));
        assert_eq!(
            read("dir = \"~/notes\"").unwrap(),
            Some(PathBuf::from("/home/me/notes"))
        );
        assert_eq!(
            read("dir = \"notes\"").unwrap(),
            Some(dir.path().join("notes"))
        );
        assert_eq!(read("").unwrap(), None);
        assert!(read("dir = 1").is_err());
        assert!(
            read("dir = ")
                .unwrap_err()
                .to_string()
                .contains("invalid config")
        );

        assert_eq!(
            read_config(&dir.path().join("missing.toml"), None).unwrap(),
            None
        );
    }
}
