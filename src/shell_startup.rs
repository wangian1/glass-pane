//! Temporary zsh startup shims; never edit the user's shell configuration.
use std::{io, path::Path};

// Let the user's normal .zshenv see their actual ZDOTDIR, then preserve any
// directory it selects while routing the upcoming .zshrc through our shim.
const ZSHENV: &str = r#"typeset _glasspane_startup_dir=$ZDOTDIR
if [[ $GLASSPANE_ZDOTDIR_SET == 1 ]]; then
  export ZDOTDIR=$GLASSPANE_USER_ZDOTDIR
else
  unset ZDOTDIR
fi
if [[ -r "${ZDOTDIR-$HOME}/.zshenv" ]]; then
  source "${ZDOTDIR-$HOME}/.zshenv"
fi
if (( ${+ZDOTDIR} )); then
  export GLASSPANE_USER_ZDOTDIR=$ZDOTDIR GLASSPANE_ZDOTDIR_SET=1
else
  export GLASSPANE_ZDOTDIR_SET=0
fi
export ZDOTDIR=$_glasspane_startup_dir
unset _glasspane_startup_dir
"#;

const ZSHRC: &str = r#"typeset _glasspane_startup_dir=$ZDOTDIR
if [[ $GLASSPANE_ZDOTDIR_SET == 1 ]]; then
  export ZDOTDIR=$GLASSPANE_USER_ZDOTDIR
else
  unset ZDOTDIR
fi
# Global startup may have based HISTFILE on our temporary directory. Keep the
# normal history destination while still allowing the user's .zshrc to override it.
if [[ ${HISTFILE-} == "$_glasspane_startup_dir/"* ]]; then
  HISTFILE="${ZDOTDIR-$HOME}/${HISTFILE#$_glasspane_startup_dir/}"
fi
unset _glasspane_startup_dir
if [[ -r "${ZDOTDIR-$HOME}/.zshrc" ]]; then
  source "${ZDOTDIR-$HOME}/.zshrc"
fi
_glasspane_summary_precmd() {
  local previous_status=$?
  "$GLASSPANE_BIN" summary
  return $previous_status
}
autoload -Uz add-zsh-hook
add-zsh-hook -d precmd _glasspane_summary_precmd 2>/dev/null
add-zsh-hook precmd _glasspane_summary_precmd
"#;

pub fn zsh_startup() -> io::Result<tempfile::TempDir> {
    let dir = tempfile::Builder::new()
        .prefix("glasspane-zsh-")
        .tempdir()?;
    std::fs::write(dir.path().join(".zshenv"), ZSHENV)?;
    std::fs::write(dir.path().join(".zshrc"), ZSHRC)?;
    Ok(dir)
}

pub fn is_zsh(shell: &Path) -> bool {
    shell.file_name().is_some_and(|s| s == "zsh")
}
