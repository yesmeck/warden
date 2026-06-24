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

  new|n  <branch>     Create a worktree at <root>/<project>/<branch>
  cd     <branch>     Change to the worktree directory for <branch>
  list|ls             List worktrees for the current repository
  remove|rm <branch>  Remove the worktree for <branch>
  shell-init          Print shell integration to enable `new`/`cd` directory changes
```

### Shell integration

`new` and `cd` print a path on stdout. To make them actually change your
shell's directory, install the wrapper function in your rc:

```sh
eval "$(warden shell-init)"
```

If you miss typing `wt`, alias it:

```sh
alias wt=warden
```

## Environment

| Variable  | Default        | Meaning                |
| --------- | -------------- | ---------------------- |
| `WT_ROOT` | `~/.worktrees` | Worktree root directory |

## License

MIT
