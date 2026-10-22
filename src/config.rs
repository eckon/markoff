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
    Report, Result,
    eyre::{WrapErr, bail, eyre},
};

use serde::Deserialize;

use crate::{cli::Command, load};

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

/// The text for `--help` / `--version`, if one is anywhere in the args. Checked before the
/// config is read, so both still work with a broken config.
pub fn info(mut args: impl Iterator<Item = String>) -> Option<String> {
    args.find_map(|arg| match arg.as_str() {
        "-h" | "--help" => Some(format!(
            "markoff: a TUI for tasks stored as markdown files\n\
             \n\
             Usage: markoff [--dir <path>] [<command>]\n\
             \n\
             Commands, without one the TUI starts:\n\
             \x20 list                   Print sections and tasks as markdown\n\
             \x20 add <section> <title>  Add a task at the end of a section, print its path\n\
             \n\
             Options:\n\
             \x20 --dir <path>   Root directory, else ${DIR_ENV}, else `general.dir` in ~/.config/{CONFIG_FILE}\n\
             \x20 -h, --help     Show this help\n\
             \x20 -V, --version  Show the version\n"
        )),
        "-V" | "--version" => Some(format!("markoff {}\n", env!("CARGO_PKG_VERSION"))),
        _ => None,
    })
}

/// A wrong argument, with the pointer to `--help` on a second line like coreutils.
pub fn usage_error(message: &str) -> Report {
    eyre!("{message}\nTry 'markoff --help' for more information.")
}

/// Resolves the root directory, the one with the sections, and the command if one is given.
///
/// Options may stand anywhere, everything else is left to [`Command::parse`], so commands never
/// see options. Root order: `--dir <path>` / `--dir=<path>`, then [`DIR_ENV`], then
/// `general.dir` of the config file. Args, env and config are parameters so this can be tested
/// without the real process.
pub fn parse_args(
    mut args: impl Iterator<Item = String>,
    env: Option<String>,
    config_dir: Option<PathBuf>,
) -> Result<(PathBuf, Option<Command>)> {
    let mut dir = None;
    let mut positionals = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--dir" {
            let Some(value) = args.next() else {
                return Err(usage_error("--dir needs a path"));
            };
            dir = Some(value);
        } else if let Some(value) = arg.strip_prefix("--dir=") {
            dir = Some(value.to_string());
        } else if arg.starts_with('-') {
            return Err(usage_error(&format!("unrecognized option '{arg}'")));
        } else {
            positionals.push(arg);
        }
    }
    // Before the root, so a wrong command is reported even without a directory.
    let command = Command::parse(&positionals)?;

    let root = match (dir.or(env), config_dir) {
        (Some(dir), _) => PathBuf::from(dir),
        (None, Some(dir)) => dir,
        (None, None) => bail!(
            "no directory given, use --dir <path>, set {DIR_ENV}, \
             or set `general.dir` in ~/.config/{CONFIG_FILE}"
        ),
    };
    Ok((root, command))
}

/// Reads the real process args, env and config file. Returns the root and the command.
pub fn from_env() -> Result<(PathBuf, Option<Command>)> {
    let var = |name| std::env::var(name).ok().filter(|v: &String| !v.is_empty());
    let home = var("HOME");
    let config = match config_path(var("XDG_CONFIG_HOME"), home.clone()) {
        Some(path) => read_config(&path, home.as_deref())?,
        None => Config::default(),
    };
    parse_args(std::env::args().skip(1), var(DIR_ENV), config.general.dir)
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

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// The root of [`parse_args`], for the tests without a command.
    fn root_dir(
        args: impl Iterator<Item = String>,
        env: Option<String>,
        config_dir: Option<PathBuf>,
    ) -> Result<PathBuf> {
        parse_args(args, env, config_dir).map(|(root, _)| root)
    }

    #[test]
    fn command_between_options() {
        let parse = |list: &[&str]| parse_args(args(list), None, None).unwrap();
        let expected = (PathBuf::from("/a"), Some(Command::List));
        assert_eq!(parse(&["list", "--dir", "/a"]), expected);
        assert_eq!(parse(&["--dir", "/a", "list"]), expected);
        assert_eq!(parse(&["--dir", "/a"]), (PathBuf::from("/a"), None));
        let add = (
            PathBuf::from("/a"),
            Some(Command::Add {
                section: "work".to_string(),
                title: "Fix it".to_string(),
            }),
        );
        assert_eq!(parse(&["add", "work", "--dir", "/a", "Fix it"]), add);
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
            "unrecognized option '--nope'\nTry 'markoff --help' for more information."
        );
    }

    #[test]
    fn info_help_and_version() {
        let help = info(args(&["--help"])).unwrap();
        assert!(help.starts_with("markoff: a TUI"));
        assert!(help.contains("Usage: markoff [--dir <path>] [<command>]"));
        assert_eq!(info(args(&["--dir", "/a", "-h"])), Some(help));
        let version = format!("markoff {}\n", env!("CARGO_PKG_VERSION"));
        assert_eq!(info(args(&["--version"])), Some(version.clone()));
        assert_eq!(info(args(&["-V"])), Some(version));
        assert_eq!(info(args(&["--dir", "/a"])), None);
        assert_eq!(info(args(&["--nope"])), None, "left to parse_args");
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
