# Smart Defaults

shako automatically detects modern CLI tools and creates aliases so you get the best available tool without any configuration.

## How It Works

At startup, shako checks which tools are installed via `$PATH`. For each detected tool, it creates a shell alias mapping the traditional command to the modern replacement. **Your config aliases always take priority** — smart defaults never override user settings.

The AI is also told which tools you have, so AI-generated commands use the modern syntax too.

## Tool Upgrades

When these tools are detected, shako aliases the classic command to the modern one:

| Modern Tool | Replaces | Default Args | What You Get |
|---|---|---|---|
| [eza](https://eza.rocks/) | `ls` | `--icons --group-directories-first` | Icons, git status, color |
| [bat](https://github.com/sharkdp/bat) | `cat` | `--style=auto` | Syntax highlighting, line numbers |
| [fd](https://github.com/sharkdp/fd) | `find` | — | Faster, simpler syntax, respects .gitignore |
| [ripgrep](https://github.com/BurntSushi/ripgrep) | `grep` | — | Faster, respects .gitignore |
| [dust](https://github.com/bootandy/dust) | `du` | — | Visual disk usage tree |
| [procs](https://github.com/dalance/procs) | `ps` | — | Colored, searchable process list |
| [sd](https://github.com/chmln/sd) | `sed` | — | Simpler regex substitution |
| [delta](https://github.com/dandavison/delta) | `diff` | — | Side-by-side, syntax-aware diffs |
| [btop](https://github.com/aristocratos/btop) | `top` | — | Modern system monitor |
| [bottom](https://github.com/ClementTsang/bottom) | `top` | — | System monitor (alternative to btop) |
| [duf](https://github.com/muesli/duf) | `df` | — | Colored disk free display |
| [doggo](https://github.com/mr-karan/doggo) | `dig` | — | Modern DNS client |
| [xh](https://github.com/ducaale/xh) | `curl` | — | Human-friendly HTTP requests |
| [tokei](https://github.com/XAMPPRocky/tokei) | `cloc` | — | Fast code statistics |

## Compound Aliases

When a prerequisite tool is detected, these convenience aliases are created:

### File Listing (requires eza)
| Alias | Expands To |
|---|---|
| `ll` | `eza -la --icons --group-directories-first` |
| `la` | `eza -a --icons --group-directories-first` |
| `lt` | `eza --tree --icons --level=2` |

### File Preview (requires bat)
| Alias | Expands To |
|---|---|
| `preview` | `bat --style=auto --color=always` |

### File Finding (requires fd)
| Alias | Expands To |
|---|---|
| `ff` | `fd --type f` (files only) |
| `fdir` | `fd --type d` (directories only) |

### Search (requires rg)
| Alias | Expands To |
|---|---|
| `rgf` | `rg -l` (filenames only) |

### Git Shortcuts (requires git)
| Alias | Expands To |
|---|---|
| `ga`   | `git add` |
| `gaa`  | `git add -A` |
| `gs`   | `git status` |
| `gb`   | `git branch` |
| `gl`   | `git log --oneline -20` |
| `gd`   | `git diff` |
| `gf`   | `git fetch` |
| `gp`   | `git push` |
| `gpl`  | `git pull` |
| `gco`  | `git checkout` |
| `gcm`  | `git commit -m` |
| `grb`  | `git rebase` |
| `gst`  | `git stash` |
| `gstp` | `git stash pop` |

### Docker Shortcuts (requires docker)
| Alias | Expands To |
|---|---|
| `dps`  | `docker ps` |
| `dex`  | `docker exec -it` |
| `dlog` | `docker logs -f` |
| `dst`  | `docker stop` |
| `drm`  | `docker rm` |
| `drmi` | `docker rmi` |
| `dimg` | `docker images` |
| `db`   | `docker build` |

### Podman Shortcuts (requires podman)
| Alias | Expands To |
|---|---|
| `pps`  | `podman ps` |
| `pex`  | `podman exec -it` |
| `plog` | `podman logs -f` |
| `ppod` | `podman pod ps` |
| `pimg` | `podman images` |
| `pb`   | `podman build` |
| `pst`  | `podman stop` |
| `prm`  | `podman rm` |
| `prmi` | `podman rmi` |
| `pnet` | `podman network ls` |
| `pvol` | `podman volume ls` |

### Kubectl Shortcuts (requires kubectl)
| Alias | Expands To |
|---|---|
| `k`   | `kubectl` |
| `kgp` | `kubectl get pods` |
| `kgs` | `kubectl get services` |
| `kgn` | `kubectl get nodes` |
| `kl`  | `kubectl logs -f` |
| `kex` | `kubectl exec -it` |
| `kaf` | `kubectl apply -f` |
| `kdf` | `kubectl delete -f` |
| `kdp` | `kubectl describe pod` |

### Terraform Shortcuts (requires terraform)
| Alias | Expands To |
|---|---|
| `tfi` | `terraform init` |
| `tfp` | `terraform plan` |
| `tfa` | `terraform apply` |
| `tfd` | `terraform destroy` |

### Cargo Shortcuts (requires cargo)
| Alias | Expands To |
|---|---|
| `cb`  | `cargo build` |
| `cr`  | `cargo run` |
| `ct`  | `cargo test` |
| `cc`  | `cargo check` |
| `ccl` | `cargo clippy` |

### npm Shortcuts (requires npm)
| Alias | Expands To |
|---|---|
| `ni` | `npm install` |
| `nr` | `npm run` |
| `nt` | `npm test` |
| `ns` | `npm start` |

## Zoxide Integration

If [zoxide](https://github.com/ajeetdsouza/zoxide) is installed:

- `cd` automatically tracks directory visits (calls `zoxide add` after each successful cd)
- `z <query>` jumps to the best-matching directory: `z proj` → `~/src/projects`
- `zi` opens an interactive picker (requires [fzf](https://github.com/junegunn/fzf))
- If zoxide is not installed, `z` falls back to regular `cd`

## Tab Completion

shako provides subcommand completions for common tools:

| Tool | Completions |
|---|---|
| `git` | 28 subcommands (add, commit, push, rebase, stash, etc.) |
| `cargo` | 19 subcommands (build, test, run, clippy, fmt, etc.) |
| `docker` / `podman` | 22 subcommands (run, ps, build, exec, compose, etc.) |
| `kubectl` / `k` | 18 subcommands (get, apply, describe, logs, etc.) |
| `make` / `gmake` | Dynamic — parses targets from `Makefile` in current directory |
| `just` | Dynamic — parses recipes from `justfile` in current directory |
| `npm` / `npx` | npm subcommands |
| `pnpm` | pnpm subcommands |
| `yarn` | yarn subcommands |
| `bun` / `bunx` | bun subcommands |
| `brew` | Homebrew subcommands |
| `go` | Go tool subcommands |
| `rustup` | rustup subcommands |
| `helm` | Helm subcommands |
| `terraform` / `tf` | Terraform subcommands |
| `sudo` | Completes the next token as a command (from `$PATH`) |
| `cd` / `z` / `pushd` | Directories only |

All completions also include:
- Every executable in `$PATH`
- All shell builtins
- File/directory path completion with symlink following
- Filenames with spaces are backslash-escaped automatically

## AI Tool Awareness

The AI receives detailed syntax guidance for each detected tool. For example, if `fd` is installed, the AI prompt includes:

> use fd instead of find. Syntax: `fd PATTERN`, `fd -e EXTENSION` for files by extension, `fd -t f` files only, `fd -t d` dirs only, `fd --size +100m` for files larger than 100 MB.

This means `find all rust files larger than 1MB` generates `fd -e rs --size +1m` instead of `find . -name "*.rs" -size +1M`.

## Discovering Active Shortcuts

Use `/shortcuts` at any time to see which shortcuts are active:

```
❯ /shortcuts podman
Shortcuts for 'podman'  (✓ active  ✗ tool not installed)

  podman
    ✓  pps       podman ps
    ✓  pex       podman exec -it
    ✓  plog      podman logs -f
    ...
```

Run `/shortcuts` with no argument to list every category. A ✗ next to an entry means the tool is not installed — the shortcut exists in shako's table but is not currently active.

## Overriding Defaults

Add entries to `[aliases]` in `~/.config/shako/config.toml` to override any smart default:

```toml
[aliases]
ls = "ls --color=auto"    # keep classic ls instead of eza
cat = "cat"                # keep classic cat instead of bat
gs = "git status -sb"     # override the default git status alias
```

Or set `smart_defaults_enabled = false` in `[behavior]` to disable auto-aliasing entirely (planned).
