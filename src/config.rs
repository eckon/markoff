//! Where markoff finds its data.

use std::path::PathBuf;

use color_eyre::{Result, eyre::bail};

/// Env var with the root directory, used when no `--dir` is given.
const DIR_ENV: &str = "MARKOFF_DIR";

/// Resolves the root directory (the one containing `todo/` and `archive/`).
///
/// Order: `--dir <path>` / `--dir=<path>`, then [`DIR_ENV`].
/// Takes args and env as parameters so it can be tested without touching the real process.
pub fn root_dir(mut args: impl Iterator<Item = String>, env: Option<String>) -> Result<PathBuf> {
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

    match dir.or(env) {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => bail!("no directory given, use --dir <path> or set {DIR_ENV}"),
    }
}

/// Reads the real process args and env.
pub fn root_dir_from_env() -> Result<PathBuf> {
    root_dir(std::env::args().skip(1), std::env::var(DIR_ENV).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn flag_wins_over_env() {
        let dir = root_dir(args(&["--dir", "/a"]), Some("/b".into())).unwrap();
        assert_eq!(dir, PathBuf::from("/a"));
        let dir = root_dir(args(&["--dir=/a"]), Some("/b".into())).unwrap();
        assert_eq!(dir, PathBuf::from("/a"));
    }

    #[test]
    fn env_fallback() {
        let dir = root_dir(args(&[]), Some("/b".into())).unwrap();
        assert_eq!(dir, PathBuf::from("/b"));
    }

    #[test]
    fn errors() {
        assert!(root_dir(args(&[]), None).is_err());
        assert!(root_dir(args(&["--dir"]), None).is_err());
        assert!(root_dir(args(&["--nope"]), None).is_err());
    }
}
