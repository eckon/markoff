//! Where markoff finds its data, and the config file.
//!
//! ```toml
//! [general]
//! dir = "~/notes/tasks"
//! ```
//!
//! [`Config`] mirrors the file. Unknown keys are an error, so a typo or a key in the wrong
//! table is caught instead of silently ignored.

use std::path::{Path, PathBuf};

use color_eyre::{
    Result,
    eyre::{WrapErr, bail},
};

use serde::Deserialize;

use crate::load;

/// Env var with the root directory, used when no `--dir` is given.
const DIR_ENV: &str = "MARKOFF_DIR";

/// The config file in the config directory, `~/.config` by default.
const CONFIG_FILE: &str = "markoff/config.toml";

/// What the config file sets, everything is optional.
#[derive(Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub general: General,
}

/// `[general]`: what is not part of a feature.
#[derive(Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct General {
    /// The root directory, used when neither `--dir` nor [`DIR_ENV`] is given.
    pub dir: Option<PathBuf>,
}

/// The root directory, the one with the sections: `--dir`, then [`DIR_ENV`], then
/// `general.dir` of the config file. Env and config are parameters so this can be tested
/// without the real process. Not clap's `env`, it would take an empty var as a value.
fn root(dir: Option<PathBuf>, env: Option<String>, config_dir: Option<PathBuf>) -> Result<PathBuf> {
    match dir.or(env.map(PathBuf::from)).or(config_dir) {
        Some(dir) => Ok(dir),
        None => bail!(
            "no directory given, use --dir <path>, set {DIR_ENV}, \
             or set `general.dir` in ~/.config/{CONFIG_FILE}"
        ),
    }
}

/// Reads the config file with the real env, and returns the root for `dir` from the args.
/// The config is read even with `dir`, so a broken one always shows.
pub fn from_env(dir: Option<PathBuf>) -> Result<PathBuf> {
    let var = |name| std::env::var(name).ok().filter(|v: &String| !v.is_empty());
    let home = var("HOME");
    let config = match config_path(var("XDG_CONFIG_HOME"), home.clone()) {
        Some(path) => read_config(&path, home.as_deref())?,
        None => Config::default(),
    };
    root(dir, var(DIR_ENV), config.general.dir)
}

/// `$XDG_CONFIG_HOME/markoff/config.toml`, falling back to `~/.config/markoff/config.toml`.
fn config_path(xdg_config_home: Option<String>, home: Option<String>) -> Option<PathBuf> {
    let base = match xdg_config_home {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(home?).join(".config"),
    };
    Some(base.join(CONFIG_FILE))
}

/// Reads the config file. A missing file is an empty config.
fn read_config(path: &Path, home: Option<&str>) -> Result<Config> {
    let Some(content) = load::read_if_exists(path)? else {
        return Ok(Config::default());
    };
    let mut config: Config =
        toml::from_str(&content).wrap_err_with(|| format!("invalid config {}", path.display()))?;
    let config_dir = path.parent().unwrap_or(Path::new("."));
    if let Some(dir) = &config.general.dir {
        config.general.dir = Some(resolve(dir, config_dir, home)?);
    }
    Ok(config)
}

/// Expands a leading `~/`, and resolves relative paths against the config file's directory
/// (not the current directory, so markoff behaves the same wherever it is started).
fn resolve(dir: &Path, config_dir: &Path, home: Option<&str>) -> Result<PathBuf> {
    let path = match dir.strip_prefix("~") {
        Ok(rest) => {
            let Some(home) = home else {
                bail!("cannot expand `~` in {}, HOME is not set", dir.display());
            };
            Path::new(home).join(rest)
        }
        Err(_) => dir.to_path_buf(),
    };
    Ok(config_dir.join(path))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn root_order() {
        let (a, c) = (PathBuf::from("/a"), PathBuf::from("/c"));
        let env = || Some("/b".to_string());
        assert_eq!(root(Some(a.clone()), env(), Some(c.clone())).unwrap(), a);
        assert_eq!(
            root(None, env(), Some(c.clone())).unwrap(),
            PathBuf::from("/b")
        );
        assert_eq!(root(None, None, Some(c.clone())).unwrap(), c);
        assert!(
            root(None, None, None)
                .unwrap_err()
                .to_string()
                .starts_with("no directory given")
        );
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

        let dir_of = |dir: &str| {
            read(&format!("[general]\ndir = {dir}"))
                .unwrap()
                .general
                .dir
        };

        assert_eq!(dir_of("\"/abs\""), Some(PathBuf::from("/abs")));
        assert_eq!(dir_of("\"~/notes\""), Some(PathBuf::from("/home/me/notes")));
        assert_eq!(dir_of("\"notes\""), Some(dir.path().join("notes")));
        assert_eq!(read("").unwrap(), Config::default());
        assert_eq!(read("[general]").unwrap(), Config::default());
        assert!(read("[general]\ndir = 1").is_err());
        assert!(
            read("dir = ")
                .unwrap_err()
                .to_string()
                .contains("invalid config")
        );

        // Unknown keys are errors that name the key: a typo, a key outside its table, or one
        // written below another table's header.
        let unknown = |content: &str| format!("{:?}", read(content).unwrap_err());
        assert!(unknown("[general]\ndri = \"/a\"").contains("unknown field `dri`"));
        assert!(unknown("dir = \"/a\"").contains("unknown field `dir`"));
        assert!(unknown("[nope]").contains("unknown field `nope`"));

        assert_eq!(
            read_config(&dir.path().join("missing.toml"), None).unwrap(),
            Config::default()
        );
    }
}
