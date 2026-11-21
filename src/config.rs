use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "glass",
    version,
    about = "Live Git dashboard with safe branch switching"
)]
pub struct Cli {
    pub path: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
    #[arg(long)]
    pub no_follow: bool,
    #[arg(long, value_name = "N")]
    pub commits: Option<usize>,
    #[arg(long)]
    pub print_config: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print a compact repository summary without starting the full-screen UI
    Summary { path: Option<PathBuf> },
    /// Print an opt-in shell integration snippet
    Init {
        #[arg(value_parser = ["zsh"])]
        shell: String,
    },
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BranchSort {
    #[default]
    Date,
    Name,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub commit_limit: usize,
    pub graph_limit: usize,
    pub debounce_ms: u64,
    pub branch_sort: BranchSort,
    pub follow_cwd: bool,
    pub show_remote_head: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            commit_limit: 50,
            graph_limit: 200,
            debounce_ms: 150,
            branch_sort: BranchSort::Date,
            follow_cwd: true,
            show_remote_head: false,
        }
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".config")))
            .map(|p| p.join("glasspane/config.toml"))
    }

    fn read_file(path: &std::path::Path) -> Result<Option<Self>> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                Ok(Some(toml::from_str(&text).with_context(|| {
                    format!("Invalid config: {}", path.display())
                })?))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("Cannot read {}", path.display())),
        }
    }

    pub fn load(cli: &Cli) -> Result<Self> {
        let mut config = Self::path()
            .map(|path| -> Result<Self> {
                if let Some(config) = Self::read_file(&path)? {
                    return Ok(config);
                }
                let legacy = path
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|p| p.join("gitpane/config.toml"));
                match legacy {
                    Some(path) => Ok(Self::read_file(&path)?.unwrap_or_default()),
                    None => Ok(Self::default()),
                }
            })
            .transpose()?
            .unwrap_or_default();
        if cli.no_follow {
            config.follow_cwd = false;
        }
        if let Some(n) = cli.commits {
            config.commit_limit = n;
        }
        if config.commit_limit == 0 || config.graph_limit == 0 || config.debounce_ms == 0 {
            bail!("commit_limit, graph_limit and debounce_ms must be greater than zero");
        }
        Ok(config)
    }
}
