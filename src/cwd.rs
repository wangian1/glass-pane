use std::path::{Path, PathBuf};

pub const ZSH_HOOK: &str = r#"# glasspane cwd tracking (last shell to change directory wins)
_glasspane_chpwd() {
  local state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/glasspane"
  command mkdir -p -- "$state_dir" || return
  print -r -- "$PWD" >| "$state_dir/cwd"
}
autoload -Uz add-zsh-hook
add-zsh-hook -d chpwd _gitpane_chpwd 2>/dev/null
add-zsh-hook -d chpwd _glasspane_chpwd 2>/dev/null
add-zsh-hook chpwd _glasspane_chpwd
_glasspane_chpwd
"#;

pub fn state_file() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".local/state")))
        .map(|p| p.join("glasspane/cwd"))
}

pub fn read(path: &Path) -> Option<PathBuf> {
    let value = std::fs::read_to_string(path).ok()?;
    // Strip only the newline written by print; spaces are valid path characters.
    let value = value.strip_suffix('\n').unwrap_or(&value);
    let path = PathBuf::from(value);
    (path.is_absolute() && path.is_dir()).then_some(path)
}
