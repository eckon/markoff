//! Where markoff finds its data, and the config file.
//!
//! The defaults are a config file of their own, [`DEFAULTS`], with every key and its comment,
//! so the docs and the code cannot drift apart. The user's file is merged over it key by key.
//! [`Config`] mirrors the merged file. Unknown keys are an error, so a typo or a key in the
//! wrong table is caught instead of silently ignored.

use std::path::{Path, PathBuf};

use color_eyre::{
    Result,
    eyre::{WrapErr, bail},
};

use serde::Deserialize;

use crate::load;

/// The config file in the config directory, `~/.config` by default.
const CONFIG_FILE: &str = "markoff/config.toml";

/// Every key with its default and its comment, the base the user's file is merged over.
const DEFAULTS: &str = include_str!("config.toml");

/// The config, [`DEFAULTS`] with the user's file over it. Only keys without a default are
/// optional, the others come from [`DEFAULTS`].
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub general: General,
    pub sections: Sections,
}

/// `[general]`: what is not part of a feature.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct General {
    /// The root directory, used when no `--dir` is given.
    pub dir: Option<PathBuf>,
}

/// `[sections]`: the section directories.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sections {
    /// Keep a `.gitkeep` in every section, see [`crate::task::GITKEEP`].
    pub gitkeep: bool,
}

/// Reads the config file with the real env, and returns the root for `dir` from the args and
/// the config. The config is read even with `dir`, so a broken one always shows.
pub fn from_env(dir: Option<PathBuf>) -> Result<(PathBuf, Config)> {
    let var = |name| std::env::var(name).ok().filter(|v: &String| !v.is_empty());
    let home = var("HOME");
    let config = match config_path(var("XDG_CONFIG_HOME"), home.clone()) {
        Some(path) => read_config(&path, home.as_deref())?,
        None => merged(toml::Table::new())?,
    };
    let Some(root) = dir.or(config.general.dir.clone()) else {
        bail!(
            "no directory given, use --dir <path> or set `general.dir` in ~/.config/{CONFIG_FILE}"
        );
    };
    Ok((root, config))
}

/// `$XDG_CONFIG_HOME/markoff/config.toml`, falling back to `~/.config/markoff/config.toml`.
fn config_path(xdg_config_home: Option<String>, home: Option<String>) -> Option<PathBuf> {
    let base = match xdg_config_home {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(home?).join(".config"),
    };
    Some(base.join(CONFIG_FILE))
}

/// Reads the config file over the defaults. A missing file is the defaults.
fn read_config(path: &Path, home: Option<&str>) -> Result<Config> {
    let content = load::read_if_exists(path)?.unwrap_or_default();
    let invalid = || format!("invalid config {}", path.display());
    let mut config = merged(content.parse().wrap_err_with(invalid)?).wrap_err_with(invalid)?;
    let config_dir = path.parent().unwrap_or(Path::new("."));
    if let Some(dir) = &config.general.dir {
        config.general.dir = Some(resolve(dir, config_dir, home)?);
    }
    Ok(config)
}

/// [`DEFAULTS`] with `user` merged over it. The merged table has no lines of the user's file,
/// so these errors name the key but no line: the user's file is parsed alone before, a syntax
/// error still has one.
fn merged(user: toml::Table) -> Result<Config, toml::de::Error> {
    let mut table: toml::Table = DEFAULTS.parse().expect("the defaults are valid TOML");
    merge(&mut table, user);
    toml::Value::Table(table).try_into()
}

/// The defaults alone, for tests that need a config.
#[cfg(test)]
pub fn defaults() -> Config {
    merged(toml::Table::new()).unwrap()
}

/// Puts the keys of `over` into `base`, into tables that both have, so a table in the user's
/// file keeps the defaults it does not set.
fn merge(base: &mut toml::Table, over: toml::Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(base)), toml::Value::Table(over)) => merge(base, over),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
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
        // The defaults parse, so a missing or empty file is them.
        let defaults = defaults();
        assert_eq!(read("").unwrap(), defaults);
        assert_eq!(read("[general]").unwrap(), defaults);
        assert!(read("[general]\ndir = 1").is_err());

        // A key set in the user's file wins, the other keys keep their defaults.
        assert!(defaults.sections.gitkeep);
        assert!(
            !read("[sections]\ngitkeep = false")
                .unwrap()
                .sections
                .gitkeep
        );
        assert!(read("[general]\ndir = \"/a\"").unwrap().sections.gitkeep);
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
            defaults
        );
    }
}
