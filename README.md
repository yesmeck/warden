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

```sh
warden help
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

| Variable        | Default        | Meaning                 |
| --------------- | -------------- | ----------------------- |
| `WARDEN_ROOT`   | `~/.worktrees` | Worktree root directory |

## License

MIT
