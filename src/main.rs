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
use std::env;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

/// Tree names used to mint a random branch when `new` is called without one.
const TREES: &[&str] = &[
    "oak", "birch", "cedar", "maple", "willow", "aspen", "alder", "beech", "elm", "fir", "hazel",
    "juniper", "larch", "linden", "pine", "poplar", "rowan", "spruce", "sycamore", "walnut", "yew",
    "ash", "hawthorn", "hickory", "magnolia", "redwood", "sequoia", "cypress", "dogwood", "hemlock",
    "ironwood", "mahogany", "mangrove", "mulberry", "olive", "teak", "banyan", "baobab", "cherry",
    "chestnut", "ebony", "ginkgo", "holly", "laurel", "myrtle", "sandalwood",
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

/// A process-random u64, seeded by the OS via the default hasher's keys.
fn random_u64() -> u64 {
    let mut h = RandomState::new().build_hasher();
    h.write_u8(0);
    h.finish()
}

/// Mint a branch name from a tree, avoiding existing branches and worktrees.
/// Falls back to a `<tree>-<hex>` suffix once the bare names are exhausted.
fn generate_branch() -> String {
    for attempt in 0..64 {
        let tree = TREES[(random_u64() as usize) % TREES.len()];
        let candidate = if attempt < TREES.len() {
            tree.to_string()
        } else {
            format!("{tree}-{:04x}", random_u64() & 0xffff)
        };
        let branch_ref = format!("refs/heads/{candidate}");
        let taken = git_check(&["show-ref", "--verify", "--quiet", &branch_ref])
            || worktree_path(&candidate).exists();
        if !taken {
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

fn cmd_cd(args: &[String]) {
    let branch = args.first().unwrap_or_else(|| die("cd: missing branch name"));
    let path = worktree_path(branch);
    if !path.is_dir() {
        die(format!("cd: no worktree at {}", path.display()));
    }
    println!("{}", path.display());
}

fn cmd_list() {
    // Restrict to the current repository (and fail clearly if not in one).
    repo_toplevel();
    if !git_run(&["worktree", "list"]) {
        die("list: git worktree list failed");
    }
}

fn cmd_remove(args: &[String]) {
    let branch = args.first().unwrap_or_else(|| die("remove: missing branch name"));
    let path = worktree_path(branch);
    if !path.exists() {
        die(format!("remove: no worktree at {}", path.display()));
    }
    if !git_run(&["worktree", "remove", &path.to_string_lossy()]) {
        die("remove: git worktree remove failed");
    }
    println!("removed {}", path.display());
}

fn cmd_shell_init() {
    // Emit a shell function that wraps this binary so `new` and `cd` can change
    // the directory of the calling shell. Capture stdout (the path) and cd to it.
    print!(
        r#"warden() {{
	case "${{1:-}}" in
		new|n|cd)
			local _w_dir
			_w_dir="$(command warden "$@")" || return
			[ -n "$_w_dir" ] && cd "$_w_dir"
			;;
		*)
			command warden "$@"
			;;
	esac
}}
"#
    );
}

fn usage() {
    print!(
        r#"Usage: warden <command> [args]

Commands:
  new|n  [branch]     Create a worktree at <root>/<project>/<branch>
                      (a tree name is generated when no branch is given)
  cd     <branch>     Change to the worktree directory for <branch>
  list|ls             List worktrees for the current repository
  remove|rm <branch>  Remove the worktree for <branch>
  shell-init          Print shell integration to enable `new`/`cd` directory changes

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
        "list" | "ls" => cmd_list(),
        "remove" | "rm" => cmd_remove(rest),
        "shell-init" => cmd_shell_init(),
        "-h" | "--help" | "help" => usage(),
        other => die(format!("unknown command: {other} (try 'warden help')")),
    }
    ExitCode::SUCCESS
}
