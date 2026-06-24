//! warden — a git worktree helper.
//!
//! Subcommands:
//!   new|n     <branch>   Create a worktree at <root>/<project>/<branch>
//!   cd        <branch>   Print the worktree path for <branch>
//!   list|ls              List worktrees for the current repository
//!   remove|rm <branch>   Remove the worktree for <branch>
//!   shell-init           Print shell integration so `new`/`cd` change directory
//!
//! To make `new` and `cd` change your shell's directory, add to your rc:
//!   eval "$(warden shell-init)"

use std::collections::hash_map::RandomState;
use std::collections::HashSet;
use std::env;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, SystemTime};

/// Tree names used to mint a random branch when `new` is called without one.
const TREES: &[&str] = &[
    "oak", "birch", "cedar", "maple", "willow", "aspen", "alder", "beech", "elm", "fir", "hazel",
    "juniper", "larch", "linden", "pine", "poplar", "rowan", "spruce", "sycamore", "walnut", "yew",
    "ash", "hawthorn", "hickory", "magnolia", "redwood", "sequoia", "cypress", "dogwood", "hemlock",
    "ironwood", "mahogany", "mangrove", "mulberry", "olive", "teak", "banyan", "baobab", "cherry",
    "chestnut", "ebony", "ginkgo", "holly", "laurel", "myrtle", "sandalwood",
];

/// Adjectives paired with a tree name to mint a random branch.
const ADJECTIVES: &[&str] = &[
    "ancient", "misty", "golden", "silent", "wild", "hidden", "frozen", "shady", "lofty", "gnarled",
    "whispering", "towering", "verdant", "crooked", "weathered", "sturdy", "fallen", "blooming",
    "mossy", "twisted", "lonely", "evergreen", "windswept", "sunlit", "dusky", "hollow", "rustling",
    "rooted", "leafy", "wandering", "tangled", "creaking", "drifting", "humble", "noble", "quiet",
];

/// Print an error to stderr and exit non-zero.
fn die(msg: impl AsRef<str>) -> ! {
    eprintln!("warden: {}", msg.as_ref());
    std::process::exit(1);
}

/// Worktree root directory: $WT_ROOT, else ~/.worktrees.
fn worktree_root() -> PathBuf {
    if let Some(root) = env::var_os("WT_ROOT").filter(|v| !v.is_empty()) {
        return PathBuf::from(root);
    }
    let home = env::var_os("HOME").unwrap_or_else(|| die("HOME is not set"));
    PathBuf::from(home).join(".worktrees")
}

/// Run `git <args>`, returning trimmed stdout on success, else None.
fn git_capture(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run `git <args>`, discarding stdout and inheriting stderr; return success.
/// Used for checks like `show-ref --quiet` where we only care about the status.
fn git_check(args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run `git <args>` inheriting both stdout and stderr; return success.
fn git_run(args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run `git <args>` routing git's stdout to *our* stderr, so our stdout stays
/// reserved for the worktree path that the shell-init wrapper captures.
fn git_progress(args: &[&str]) -> bool {
    let mut child = match Command::new("git")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    if let Some(mut out) = child.stdout.take() {
        let _ = io::copy(&mut out, &mut io::stderr());
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

/// Ensure we're inside a git repository, returning its top-level directory.
fn repo_toplevel() -> String {
    git_capture(&["rev-parse", "--show-toplevel"])
        .unwrap_or_else(|| die("not inside a git repository"))
}

/// Derive the project name. Use the common git dir's parent so every worktree
/// of a repo maps to the same project bucket.
fn project_name() -> String {
    let toplevel = repo_toplevel();
    if let Some(common) = git_capture(&["rev-parse", "--path-format=absolute", "--git-common-dir"]) {
        if !common.is_empty() {
            if let Some(name) = Path::new(&common).parent().and_then(Path::file_name) {
                return name.to_string_lossy().into_owned();
            }
        }
    }
    Path::new(&toplevel)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| die("could not determine project name"))
}

/// Compute the worktree path for a branch in the current project.
fn worktree_path(branch: &str) -> PathBuf {
    worktree_root().join(project_name()).join(branch)
}

/// The repository's main worktree (parent of the shared git common dir).
fn main_worktree() -> Option<PathBuf> {
    let common = git_capture(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    Path::new(&common).parent().map(Path::to_path_buf)
}

/// A process-random u64, seeded by the OS via the default hasher's keys.
fn random_u64() -> u64 {
    let mut h = RandomState::new().build_hasher();
    h.write_u8(0);
    h.finish()
}

/// Collect every branch name already in use — local heads, remote-tracking
/// branches, and tags — so a generated name can't clash with any of them.
fn used_branch_names() -> HashSet<String> {
    let mut names = HashSet::new();
    if let Some(out) = git_capture(&[
        "for-each-ref",
        "--format=%(refname:short)",
        "refs/heads",
        "refs/remotes",
        "refs/tags",
    ]) {
        for name in out.lines().map(str::trim).filter(|s| !s.is_empty()) {
            names.insert(name.to_string());
            // Also record the bare branch name behind a remote prefix
            // (e.g. `origin/misty-cedar` -> `misty-cedar`).
            if let Some((_, bare)) = name.split_once('/') {
                names.insert(bare.to_string());
            }
        }
    }
    names
}

/// Mint an `<adjective>-<tree>` branch name that is not already in use by any
/// branch (local or remote), tag, or worktree. Falls back to an extra `-<hex>`
/// suffix if collisions persist.
fn generate_branch() -> String {
    let used = used_branch_names();
    for attempt in 0..64 {
        let adj = ADJECTIVES[(random_u64() as usize) % ADJECTIVES.len()];
        let tree = TREES[(random_u64() as usize) % TREES.len()];
        let candidate = if attempt < 32 {
            format!("{adj}-{tree}")
        } else {
            format!("{adj}-{tree}-{:04x}", random_u64() & 0xffff)
        };
        if !used.contains(&candidate) && !worktree_path(&candidate).exists() {
            return candidate;
        }
    }
    format!("tree-{:08x}", random_u64() as u32)
}

fn cmd_new(args: &[String]) {
    let branch = match args.first() {
        Some(b) => b.clone(),
        None => {
            let b = generate_branch();
            eprintln!("warden: no branch given, using '{b}'");
            b
        }
    };
    let branch = branch.as_str();
    let path = worktree_path(branch);

    if path.exists() {
        die(format!("new: path already exists: {}", path.display()));
    }
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            die(format!("new: could not create {}: {e}", parent.display()));
        }
    }

    let path_str = path.to_string_lossy();
    let branch_ref = format!("refs/heads/{branch}");

    let ok = if git_check(&["show-ref", "--verify", "--quiet", &branch_ref]) {
        // Branch already exists: check it out into the new worktree.
        git_progress(&["worktree", "add", &path_str, branch])
    } else {
        // Create a new branch off the current HEAD.
        git_progress(&["worktree", "add", "-b", branch, &path_str])
    };

    if !ok {
        die("new: git worktree add failed");
    }

    // stdout carries only the path so the shell-init wrapper can cd into it.
    println!("{path_str}");
}

/// Path of the worktree currently checked out on `branch`, if any. Covers the
/// main worktree and any linked worktree, wherever it lives.
fn worktree_for_branch(branch: &str) -> Option<String> {
    let raw = git_capture(&["worktree", "list", "--porcelain"])?;
    let mut current: Option<String> = None;
    for line in raw.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            current = Some(p.to_string());
        } else if let Some(b) = line.strip_prefix("branch ") {
            let name = b.strip_prefix("refs/heads/").unwrap_or(b);
            if name == branch {
                return current;
            }
        }
    }
    None
}

fn cmd_cd(args: &[String]) {
    // No branch: jump to the main worktree.
    let Some(branch) = args.first() else {
        repo_toplevel(); // fail clearly if we're not in a repo
        let main = main_worktree().unwrap_or_else(|| die("cd: cannot locate main worktree"));
        println!("{}", main.display());
        return;
    };

    // Prefer the worktree actually checked out on this branch (incl. main).
    if let Some(path) = worktree_for_branch(branch) {
        println!("{path}");
        return;
    }

    // Fall back to the conventional warden path for this branch.
    let path = worktree_path(branch);
    if path.is_dir() {
        println!("{}", path.display());
        return;
    }

    die(format!("cd: no worktree for branch '{branch}'"));
}

/// Age of a path from its filesystem creation time, if available.
fn created_age(path: &str) -> Option<Duration> {
    let created = std::fs::metadata(path).ok()?.created().ok()?;
    SystemTime::now().duration_since(created).ok()
}

/// Render a duration as a coarse, git-style relative age ("3 days ago").
fn humanize_age(d: Duration) -> String {
    const MIN: u64 = 60;
    const HOUR: u64 = 60 * MIN;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;
    const MONTH: u64 = 30 * DAY;
    const YEAR: u64 = 365 * DAY;

    let s = d.as_secs();
    let (n, unit) = if s < MIN {
        return "just now".to_string();
    } else if s < HOUR {
        (s / MIN, "minute")
    } else if s < DAY {
        (s / HOUR, "hour")
    } else if s < WEEK {
        (s / DAY, "day")
    } else if s < MONTH {
        (s / WEEK, "week")
    } else if s < YEAR {
        (s / MONTH, "month")
    } else {
        (s / YEAR, "year")
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

fn cmd_list(args: &[String]) {
    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");

    // Restrict to the current repository (and fail clearly if not in one).
    repo_toplevel();
    let raw = git_capture(&["worktree", "list", "--porcelain"])
        .unwrap_or_else(|| die("list: git worktree list failed"));

    // Parse the porcelain blocks into (path, short-sha, label) rows.
    let mut rows: Vec<(String, String, String, String)> = Vec::new();
    let mut path = String::new();
    let mut sha = String::new();
    let mut label = String::new();
    let mut flush = |path: &mut String, sha: &mut String, label: &mut String| {
        if path.is_empty() {
            return;
        }
        let age = created_age(path).map(humanize_age).unwrap_or_else(|| "?".to_string());
        rows.push((
            std::mem::take(path),
            std::mem::take(sha),
            std::mem::take(label),
            age,
        ));
    };

    for line in raw.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            flush(&mut path, &mut sha, &mut label);
            path = p.to_string();
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            sha = h.chars().take(8).collect();
        } else if let Some(b) = line.strip_prefix("branch ") {
            let name = b.strip_prefix("refs/heads/").unwrap_or(b);
            label = format!("[{name}]");
        } else if line == "detached" {
            label = "(detached HEAD)".to_string();
        } else if line == "bare" {
            label = "(bare)".to_string();
        }
    }
    flush(&mut path, &mut sha, &mut label);

    // Columns in order: branch, head, date — and the folder only with -v.
    let label_w = rows.iter().map(|r| r.2.len()).max().unwrap_or(0);
    let age_w = rows.iter().map(|r| r.3.len()).max().unwrap_or(0);
    for (path, sha, label, age) in &rows {
        if verbose {
            println!("{label:<label_w$}  {sha}  {age:<age_w$}  {path}");
        } else {
            println!("{label:<label_w$}  {sha}  {age}");
        }
    }
}

/// Print the branch name of every worktree (including main), one per line.
/// Used by the `rm`/`cd` shell completion emitted by `shell-init`.
fn cmd_branches() {
    let Some(raw) = git_capture(&["worktree", "list", "--porcelain"]) else {
        return;
    };
    for line in raw.lines() {
        if let Some(b) = line.strip_prefix("branch ") {
            println!("{}", b.strip_prefix("refs/heads/").unwrap_or(b));
        }
    }
}

fn cmd_remove(args: &[String]) {
    let force = args.iter().any(|a| a == "-f" || a == "--force");
    let branch = args.iter().find(|a| !a.starts_with('-'));

    // With a branch, remove that worktree. Without one, remove the worktree we
    // are currently standing in and report the main worktree as the place for
    // the shell wrapper to cd into afterward.
    let (path, land) = match branch {
        Some(b) => (worktree_path(b), None),
        None => {
            let here = PathBuf::from(repo_toplevel());
            let main = main_worktree().unwrap_or_else(|| die("remove: cannot locate main worktree"));
            if here == main {
                die("remove: already in the main worktree; nothing to remove");
            }
            (here, Some(main))
        }
    };

    if !path.exists() {
        die(format!("remove: no worktree at {}", path.display()));
    }

    let path_str = path.to_string_lossy().into_owned();
    // Run git from the landing dir so removing the current worktree is safe.
    let land_str = land.as_ref().map(|p| p.to_string_lossy().into_owned());

    let mut git_args: Vec<&str> = Vec::new();
    if let Some(dir) = land_str.as_deref() {
        git_args.push("-C");
        git_args.push(dir);
    }
    git_args.push("worktree");
    git_args.push("remove");
    if force {
        // `--force` lets git remove a worktree with uncommitted changes.
        git_args.push("--force");
    }
    git_args.push(&path_str);

    if !git_run(&git_args) {
        die("remove: git worktree remove failed (use --force to discard changes)");
    }

    // Confirmation goes to stderr; only a cd target (if any) goes to stdout so
    // the shell wrapper can move out of the directory it just deleted.
    eprintln!("warden: removed {}", path.display());
    if let Some(dir) = land {
        println!("{}", dir.display());
    }
}

/// Shell integration: a wrapper function so `new`/`cd` change the calling
/// shell's directory, plus tab-completion for subcommands and branch names
/// (zsh and bash). Printed verbatim, so shell braces need no escaping.
const SHELL_INIT: &str = r#"warden() {
	case "${1:-}" in
		new|n|cd|rm|remove)
			# These may print a directory on stdout (a new/target worktree, or
			# the main worktree after removing the current one) — cd into it.
			local _w_dir
			_w_dir="$(command warden "$@")" || return
			[ -n "$_w_dir" ] && cd "$_w_dir"
			;;
		*)
			command warden "$@"
			;;
	esac
}

if [ -n "${ZSH_VERSION:-}" ]; then
	_warden() {
		local -a _subs
		_subs=(new n cd list ls remove rm shell-init help)
		if (( CURRENT == 2 )); then
			compadd -- $_subs
			return
		fi
		case ${words[2]} in
			rm|remove|cd)
				local -a _branches
				_branches=(${(f)"$(command warden __branches 2>/dev/null)"})
				compadd -- $_branches
				;;
		esac
	}
	compdef _warden warden 2>/dev/null
elif [ -n "${BASH_VERSION:-}" ]; then
	_warden_bash() {
		local cur="${COMP_WORDS[COMP_CWORD]}"
		if [ "$COMP_CWORD" -eq 1 ]; then
			COMPREPLY=( $(compgen -W "new n cd list ls remove rm shell-init help" -- "$cur") )
			return
		fi
		case "${COMP_WORDS[1]}" in
			rm|remove|cd)
				COMPREPLY=( $(compgen -W "$(command warden __branches 2>/dev/null)" -- "$cur") )
				;;
		esac
	}
	complete -F _warden_bash warden
fi
"#;

fn cmd_shell_init() {
    print!("{SHELL_INIT}");
}

fn usage() {
    print!(
        r#"Usage: warden <command> [args]

Commands:
  new|n  [branch]     Create a worktree at <root>/<project>/<branch>
                      (a tree name is generated when no branch is given)
  cd     [branch]     Change to the worktree for <branch> (any worktree,
                      including main); with no branch, go to the main worktree
  list|ls  [-v]       List worktrees (branch, head, age); -v also shows the folder
  remove|rm [branch]  Remove the worktree for <branch>, or the current one if
                      omitted (-f/--force to discard uncommitted changes)
  shell-init          Print shell integration (directory changes + completion)

Environment:
  WT_ROOT   Worktree root directory (default: ~/.worktrees)

To enable directory changes from `new` and `cd`, add to your shell rc:
  eval "$(warden shell-init)"
"#
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(sub) = args.first() else {
        usage();
        return ExitCode::FAILURE;
    };
    let rest = &args[1..];

    match sub.as_str() {
        "new" | "n" => cmd_new(rest),
        "cd" => cmd_cd(rest),
        "list" | "ls" => cmd_list(rest),
        "remove" | "rm" => cmd_remove(rest),
        "shell-init" => cmd_shell_init(),
        "__branches" => cmd_branches(),
        "-h" | "--help" | "help" => usage(),
        other => die(format!("unknown command: {other} (try 'warden help')")),
    }
    ExitCode::SUCCESS
}
