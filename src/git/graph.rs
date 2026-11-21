use std::{path::Path, process::Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneColor {
    Cyan,
    Magenta,
    Yellow,
    Blue,
    Green,
    Red,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCell {
    pub symbol: char,
    pub color: Option<LaneColor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCommit {
    pub hash: String,
    pub decorations: Vec<String>,
    pub subject: String,
}

impl GraphCommit {
    pub fn is_head(&self) -> bool {
        self.decorations
            .iter()
            .any(|name| name == "HEAD" || name.starts_with("HEAD -> "))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphLine {
    pub lanes: Vec<GraphCell>,
    pub commit: Option<GraphCommit>,
}

pub trait GraphProvider {
    fn load(&self, root: &Path, limit: usize) -> Result<Vec<GraphLine>, String>;
}

pub struct GitCliGraph;

impl GraphProvider for GitCliGraph {
    fn load(&self, root: &Path, limit: usize) -> Result<Vec<GraphLine>, String> {
        let output = Command::new("git")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .arg("--no-pager")
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "core.fsmonitor=false",
                "-c",
                "log.graphColors=cyan,magenta,yellow,blue,green,red",
                "log",
                "--graph",
                "--all",
                "--decorate=short",
                "--color=always",
                "--no-show-signature",
                "--format=%x1f%h%x1f%D%x1f%s",
                "-n",
            ])
            .arg(limit.to_string())
            .output()
            .map_err(|e| format!("Git graph unavailable: {e}. Install git on PATH."))?;
        if !output.status.success() {
            return Err(format!(
                "Git graph: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(parse(&String::from_utf8_lossy(&output.stdout)))
    }
}

// Decode Git's lane colors into data. No ANSI sequences reach the renderer.
// Git assigns these colors while routing edges, including lanes that move columns.
fn parse_lanes(text: &str) -> Vec<GraphCell> {
    let mut cells = vec![];
    let mut chars = text.chars().peekable();
    let mut color = None;
    while let Some(symbol) = chars.next() {
        if symbol == '\x1b' {
            if chars.next() == Some('[') {
                let mut codes = String::new();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        if c == 'm' {
                            for code in codes.split(';') {
                                color = match code {
                                    "0" | "" | "39" => None,
                                    "31" => Some(LaneColor::Red),
                                    "32" => Some(LaneColor::Green),
                                    "33" => Some(LaneColor::Yellow),
                                    "34" => Some(LaneColor::Blue),
                                    "35" => Some(LaneColor::Magenta),
                                    "36" => Some(LaneColor::Cyan),
                                    _ => color,
                                };
                            }
                        }
                        break;
                    }
                    codes.push(c);
                }
            }
        } else if matches!(symbol, ' ' | '*' | '|' | '/' | '\\' | '-' | '.' | '_') {
            cells.push(GraphCell { symbol, color });
        }
    }
    cells
}

pub fn parse(text: &str) -> Vec<GraphLine> {
    text.lines()
        .map(|line| {
            let (lanes, commit) = match line.split_once('\x1f') {
                Some((lanes, text)) => {
                    let mut fields = text.splitn(3, '\x1f');
                    let hash = fields.next().unwrap_or_default().to_string();
                    let decorations = fields
                        .next()
                        .unwrap_or_default()
                        .split(", ")
                        .filter(|name| !name.is_empty())
                        .map(str::to_string)
                        .collect();
                    let subject = fields.next().unwrap_or_default().to_string();
                    (
                        lanes,
                        Some(GraphCommit {
                            hash,
                            decorations,
                            subject,
                        }),
                    )
                }
                None => (line, None),
            };
            GraphLine {
                lanes: parse_lanes(lanes),
                commit,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_merge_topology_and_separates_commit_fields() {
        let rows = parse(include_str!("../../tests/fixtures/graph.txt"));
        assert_eq!(
            rows[0].lanes.iter().map(|c| c.symbol).collect::<String>(),
            "*   "
        );
        let commit = rows[0].commit.as_ref().unwrap();
        assert_eq!(commit.hash, "abc1234");
        assert_eq!(commit.subject, "Merge topic");
        assert!(commit.is_head());
        assert_eq!(
            rows[1].lanes.iter().map(|c| c.symbol).collect::<String>(),
            "|\\  "
        );
        assert!(rows[1].commit.is_none());
        assert_eq!(
            rows[2].commit.as_ref().unwrap().decorations,
            ["origin/topic"]
        );
        assert_eq!(parse(""), vec![]);
    }

    #[test]
    fn decodes_edge_colors_without_retaining_terminal_escapes() {
        let rows = parse("\x1b[36m|\x1b[m \x1b[35m/\x1b[m \n");
        assert_eq!(
            rows[0].lanes[0],
            GraphCell {
                symbol: '|',
                color: Some(LaneColor::Cyan)
            }
        );
        assert_eq!(
            rows[0].lanes[2],
            GraphCell {
                symbol: '/',
                color: Some(LaneColor::Magenta)
            }
        );
        assert_eq!(rows[0].lanes[1].color, None);
    }

    #[test]
    fn subject_parentheses_and_separators_do_not_become_branch_labels() {
        let rows = parse("* \x1fabcd123\x1f\x1fFix (parser), handle \x1f input");
        let commit = rows[0].commit.as_ref().unwrap();
        assert!(commit.decorations.is_empty());
        assert_eq!(commit.subject, "Fix (parser), handle \x1f input");
        assert!(!commit.is_head());
    }
}
