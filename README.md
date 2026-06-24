# warden

A small git worktree warden — patrol, plant, and clear worktrees under one roof.

Worktrees live at `<root>/<project>/<branch>`, where `<root>` defaults to
`~/.worktrees` and `<project>` is derived from the repository so every worktree
of a repo lands in the same bucket.

## Install

```sh
cargo install --path .
# or
cargo build --release && cp target/release/warden ~/.local/bin/
```

## Usage

```
warden <command> [args]

  new|n  [branch]     Create a worktree at <root>/<project>/<branch>
                      (a random <adjective>-<tree> name, e.g. misty-cedar,
                      is generated if you omit <branch>)
  cd     [branch]     Change to the worktree for <branch> (any worktree,
                      including main); with no branch, go to the main worktree
  list|ls  [-v]       List worktrees (branch, head, age); -v also shows the folder
  remove|rm [branch]  Remove the worktree for <branch>, or the current one if
                      omitted (-f/--force to discard uncommitted changes)
  shell-init          Print shell integration to enable `new`/`cd` directory changes
```

### Shell integration

`new` and `cd` print a path on stdout. To make them actually change your
shell's directory, install the wrapper function in your rc:

```sh
eval "$(warden shell-init)"
```

This also installs tab-completion (zsh and bash): subcommands complete at the
first position, and `rm`/`cd` complete the current repo's worktree branches.

## Environment

| Variable  | Default        | Meaning                |
| --------- | -------------- | ---------------------- |
| `WT_ROOT` | `~/.worktrees` | Worktree root directory |

## License

MIT
