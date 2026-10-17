//! Where markoff finds its data, and the config file.
//!
//! ```toml
//! dir = "~/notes/todo"
//!
//! # Not hardened yet, may change.
//! [experimental]
//! daily = "daily.md"
//! ```

use std::path::{Path, PathBuf};

use color_eyre::{
    Result,
    eyre::{WrapErr, bail},
};

use crate::load;

/// Env var with the root directory, used when no `--dir` is given.
const DIR_ENV: &str = "MARKOFF_DIR";

/// What the config file sets, everything is optional.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// The root directory, used when neither `--dir` nor [`DIR_ENV`] is given.
    pub dir: Option<PathBuf>,
    /// `[experimental] daily`: file name of the daily checklist in the root, see
    /// [`crate::daily`]. Without it there is no checklist.
    pub daily: Option<String>,
}

/// The text for `--help` / `--version`, if one is anywhere in the args. Checked before the
/// config is read, so both still work with a broken config.
pub fn info(mut args: impl Iterator<Item = String>) -> Option<String> {
    args.find_map(|arg| match arg.as_str() {
        "-h" | "--help" => Some(format!(
            "markoff: a TUI for todos stored as markdown files\n\
             \n\
             Usage: markoff [--dir <path>]\n\
             \n\
             \x20 --dir <path>   Root directory, else ${DIR_ENV}, else `dir` in ~/.config/markoff/config.toml\n\
             \x20 -h, --help     Show this help\n\
             \x20 -V, --version  Show the version\n"
        )),
        "-V" | "--version" => Some(format!("markoff {}\n", env!("CARGO_PKG_VERSION"))),
        _ => None,
    })
}

/// Resolves the root directory, the one with the sections.
///
/// Order: `--dir <path>` / `--dir=<path>`, then [`DIR_ENV`], then `dir` of the config file.
/// Args, env and config are parameters so this can be tested without the real process.
pub fn root_dir(
    mut args: impl Iterator<Item = String>,
    env: Option<String>,
    config_dir: Option<PathBuf>,
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
            bail!("unknown argument: {arg}, see --help");
        }
    }

    if let Some(dir) = dir.or(env) {
        return Ok(PathBuf::from(dir));
    }
    match config_dir {
        Some(dir) => Ok(dir),
        None => bail!(
            "no directory given, use --dir <path>, set {DIR_ENV}, \
             or set `dir` in ~/.config/markoff/config.toml"
        ),
    }
}

/// Reads the real process args, env and config file. Returns the root and the config.
pub fn from_env() -> Result<(PathBuf, Config)> {
    let var = |name| std::env::var(name).ok().filter(|v: &String| !v.is_empty());
    let home = var("HOME");
    let config = match config_path(var("XDG_CONFIG_HOME"), home.clone()) {
        Some(path) => read_config(&path, home.as_deref())?,
        None => Config::default(),
    };
    let root = root_dir(std::env::args().skip(1), var(DIR_ENV), config.dir.clone())?;
    Ok((root, config))
}

/// `$XDG_CONFIG_HOME/markoff/config.toml`, falling back to `~/.config/markoff/config.toml`.
fn config_path(xdg_config_home: Option<String>, home: Option<String>) -> Option<PathBuf> {
    let base = match xdg_config_home {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(home?).join(".config"),
    };
    Some(base.join("markoff").join("config.toml"))
}

/// Reads the config file. A missing file is an empty config.
fn read_config(path: &Path, home: Option<&str>) -> Result<Config> {
    let Some(content) = load::read_if_exists(path)? else {
        return Ok(Config::default());
    };
    let table: toml::Table = content
        .parse()
        .wrap_err_with(|| format!("invalid config {}", path.display()))?;
    let string = |table: &toml::Table, key: &str| match table.get(key) {
        None => Ok(None),
        Some(value) => match value.as_str() {
            Some(text) => Ok(Some(text.to_string())),
            None => bail!("`{key}` in {} must be a string", path.display()),
        },
    };

    let config_dir = path.parent().unwrap_or(Path::new("."));
    let dir = match string(&table, "dir")? {
        Some(dir) => Some(resolve(&dir, config_dir, home)?),
        None => None,
    };
    let daily = match table.get("experimental") {
        None => None,
        Some(value) => match value.as_table() {
            Some(experimental) => string(experimental, "daily")?,
            None => bail!("`experimental` in {} must be a table", path.display()),
        },
    };
    Ok(Config { dir, daily })
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
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn flag_wins_over_env() {
        let dir = root_dir(args(&["--dir", "/a"]), Some("/b".into()), None).unwrap();
        assert_eq!(dir, PathBuf::from("/a"));
        let dir = root_dir(args(&["--dir=/a"]), Some("/b".into()), None).unwrap();
        assert_eq!(dir, PathBuf::from("/a"));
    }

    #[test]
    fn env_fallback() {
        let dir = root_dir(args(&[]), Some("/b".into()), None).unwrap();
        assert_eq!(dir, PathBuf::from("/b"));
    }

    #[test]
    fn config_fallback() {
        let config = Some(PathBuf::from("/c"));
        assert_eq!(
            root_dir(args(&[]), None, config.clone()).unwrap(),
            PathBuf::from("/c")
        );
        let dir = root_dir(args(&["--dir", "/a"]), None, config).unwrap();
        assert_eq!(dir, PathBuf::from("/a"));
    }

    #[test]
    fn errors() {
        assert!(root_dir(args(&[]), None, None).is_err());
        assert!(root_dir(args(&["--dir"]), None, None).is_err());
        assert_eq!(
            root_dir(args(&["--nope"]), None, None)
                .unwrap_err()
                .to_string(),
            "unknown argument: --nope, see --help"
        );
    }

    #[test]
    fn info_help_and_version() {
        let help = info(args(&["--help"])).unwrap();
        assert!(help.starts_with("markoff: a TUI"));
        assert!(help.contains("Usage: markoff [--dir <path>]"));
        assert_eq!(info(args(&["--dir", "/a", "-h"])), Some(help));
        let version = format!("markoff {}\n", env!("CARGO_PKG_VERSION"));
        assert_eq!(info(args(&["--version"])), Some(version.clone()));
        assert_eq!(info(args(&["-V"])), Some(version));
        assert_eq!(info(args(&["--dir", "/a"])), None);
        assert_eq!(info(args(&["--nope"])), None, "left to root_dir");
    }

    #[test]
    fn read_config_experimental_daily() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let read = |content: &str| {
            fs::write(&path, content).unwrap();
            read_config(&path, None)
        };

        let config = read("[experimental]\ndaily = \"daily.md\"").unwrap();
        assert_eq!(config.daily.as_deref(), Some("daily.md"));
        assert_eq!(read("[experimental]").unwrap().daily, None);
        assert!(read("[experimental]\ndaily = true").is_err());
        assert!(read("experimental = 1").is_err());
        assert_eq!(
            read("daily = \"daily.md\"").unwrap().daily,
            None,
            "only under [experimental]"
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

        let dir_of = |content: &str| read(content).unwrap().dir;

        assert_eq!(dir_of("dir = \"/abs\""), Some(PathBuf::from("/abs")));
        assert_eq!(
            dir_of("dir = \"~/notes\""),
            Some(PathBuf::from("/home/me/notes"))
        );
        assert_eq!(dir_of("dir = \"notes\""), Some(dir.path().join("notes")));
        assert_eq!(read("").unwrap(), Config::default());
        assert!(read("dir = 1").is_err());
        assert!(
            read("dir = ")
                .unwrap_err()
                .to_string()
                .contains("invalid config")
        );

        assert_eq!(
            read_config(&dir.path().join("missing.toml"), None).unwrap(),
            Config::default()
        );
    }
}
