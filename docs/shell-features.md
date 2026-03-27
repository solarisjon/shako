# Shell Features

## Builtins

| Command | Description |
|---|---|
| `cd [dir]` | Change directory. Tracks visits via zoxide if installed. `cd -` returns to previous directory. |
| `z <query>` | Zoxide smart jump — `z proj` jumps to your most-visited projects directory |
| `zi` | Interactive directory picker (zoxide + fzf) |
| `exit` | Exit the shell |
| `export KEY=val` | Set environment variable (POSIX style) |
| `set -x KEY val` | Set/export variable (fish style). Also: `-gx` (global export), `-e` (erase) |
| `set` | List all environment variables |
| `unset KEY` | Remove environment variable |
| `alias name=value` | Define alias. `alias` with no args lists all aliases |
| `unalias name` | Remove alias. `-a` clears all |
| `abbr` | Manage abbreviations (fish-style). `abbr add name value`, `abbr -e name`, `abbr` to list |
| `echo [args]` | Print arguments to stdout with escape support |
| `read [-p prompt] VAR` | Read a line from stdin into a variable |
| `test` / `[` | Conditional evaluation (`-f file`, `-d dir`, `-z str`, `-n str`, `str1 = str2`, numeric comparisons) |
| `pwd` | Print current working directory |
| `pushd [dir]` | Push directory onto stack and cd to it |
| `popd` | Pop directory from stack and cd to it |
| `dirs` | Display directory stack |
| `history [N]` | Show last N history entries (default 25) |
| `source file` | Load aliases, exports, set commands, and functions from file |
| `type name` | Show how a name resolves: builtin → function → alias → PATH |
| `command name` | Run a command bypassing aliases and functions |
| `function name() { body }` | Define a shell function (`;` separates statements) |
| `functions` | List all defined functions |
| `jobs` | List background jobs |
| `fg [%N]` | Bring job N to foreground |
| `bg [%N]` | Resume stopped job N in background |
| `disown [%N]` | Remove job from shell tracking so it survives shell exit |
| `wait [%N]` | Wait for background job(s) to finish |
| `true` / `false` | Return exit code 0 / 1 (for scripting) |
| `return [N]` | Return from a function with exit code N |
| `break` / `continue` | Loop control flow |
| `local VAR=value` | Set a function-local variable |
| `fish-import` | Import fish shell configuration (aliases, env, functions) |

## Pipes

Standard Unix pipe syntax:

```bash
ls | grep foo | wc -l
cat file.txt | sort | uniq -c | sort -rn
```

All processes in a pipeline share a process group and receive terminal signals together.

## Redirects

### Standard Redirects

```bash
echo hello > file.txt         # stdout to file (overwrite)
echo hello >> file.txt        # stdout to file (append)
sort < input.txt              # stdin from file
echo hello >file.txt          # no space also works
```

### Stderr Redirects

```bash
make 2> errors.log            # stderr to file
make 2>> errors.log           # stderr to file (append)
make 2>&1                     # merge stderr into stdout
make > out.log 2> err.log     # separate stdout and stderr
make 2>&1 | grep error        # pipe combined output
```

## Command Chaining

```bash
mkdir foo && cd foo            # run second only if first succeeds
make || echo "build failed"   # run second only if first fails
cmd1; cmd2; cmd3              # run all regardless of exit codes
```

## Quoting and Expansion

### Quoting

```bash
echo "hello world"            # double quotes: variable expansion inside
echo 'no $expansion'          # single quotes: literal, no expansion
echo hello\ world             # backslash escapes the space
```

### Variable Expansion

```bash
echo $HOME                    # environment variable
echo ${USER}                  # braced form
echo $?                       # last exit code
```

### Command Substitution

```bash
echo $(date)                  # modern syntax
echo `whoami`                 # backtick syntax
echo $(echo $(pwd))           # nested substitution
cd $(dirname $file)           # use in arguments
```

### Glob Expansion

```bash
ls *.rs                       # matches all .rs files
cat src/**/*.rs               # recursive glob (if supported)
echo "*.rs"                   # suppressed inside quotes
```

### Tilde Expansion

```bash
cd ~/projects                 # expands to $HOME/projects
ls ~/.config/shako/           # works everywhere
```

### Brace Expansion

```bash
echo {a,b,c}                  # → a b c
echo file.{txt,md,rs}         # → file.txt file.md file.rs
echo {1..5}                   # → 1 2 3 4 5
echo {a..e}                   # → a b c d e
echo {01..10}                 # → 01 02 03 04 05 06 07 08 09 10 (zero-padded)
echo {5..1}                   # → 5 4 3 2 1 (reverse)
mkdir -p src/{api,db,web}     # creates 3 directories
```

### Herestring

```bash
grep foo <<< "hello foo bar"  # pipe string to stdin
wc -w <<< "$MY_VAR"          # works with variables
cat <<< hello                 # no-space form also works
```

### Parameter Expansion

```bash
${VAR:-default}               # use default if VAR is unset or empty
${VAR:+alternate}             # use alternate if VAR is set and non-empty
${VAR:?error message}         # error if VAR is unset or empty
${VAR:=default}               # assign default if unset or empty
${#VAR}                       # string length
${VAR#pattern}                # remove shortest prefix match
${VAR##pattern}               # remove longest prefix match
${VAR%pattern}                # remove shortest suffix match
${VAR%%pattern}               # remove longest suffix match
${VAR/old/new}                # replace first match
${VAR//old/new}               # replace all matches
```

### History Expansion

```bash
!!                            # repeat last command
sudo !!                       # run last command with sudo
echo !$                       # last argument of last command
```

When history expansion triggers, the expanded command is shown in gray before execution.

## Background Jobs

```bash
sleep 100 &                   # start in background
jobs                          # list running jobs
fg %1                         # bring job 1 to foreground
bg %1                         # resume stopped job in background
```

Background jobs get their own process group. `Ctrl-C` only reaches the foreground process.

## Control Flow

### Conditionals

```fish
if test -f config.toml
    echo "config found"
else if test -f config.json
    echo "JSON config found"
else
    echo "no config"
end
```

### Loops

```fish
for f in *.rs
    echo $f
end

for i in {1..5}
    echo "iteration $i"
end

while test $count -lt 10
    echo $count
    set count (math $count + 1)
end
```

### Loop Control

```fish
for f in *.txt
    if test "$f" = "skip.txt"
        continue
    end
    if test "$f" = "stop.txt"
        break
    end
    echo $f
end
```

### Local Variables

```bash
function greet() {
    local name="$1"
    echo "hello $name"
}
```

## Multiline Input

```bash
echo hello \                  # trailing backslash continues
world                         # on the next line

echo "this is                 # unclosed quote continues
a multiline string"           # until the quote is closed
```

The prompt changes to `... ` for continuation lines.

## Non-Interactive Mode

Run a single command and exit without starting the REPL:

```bash
shako -c "ls -la"
shako -c "git status && git diff"
shako -c "make build"
```

Useful in scripts or when you need shako's parsing (quoting, expansion, aliases) without an interactive session.

## Functions

Define functions inline or in files:

```bash
function greet() { echo "hello $1" }
greet world                   # → hello world

function deploy() { git push; ssh prod "cd /app && git pull" }
deploy                        # runs both commands
```

Functions in `~/.config/shako/functions/` are autoloaded on first call.

Multiple statements are separated by `;`.

## Syntax Highlighting

As you type, shako colors the entire line:

| Element | Color |
|---|---|
| Valid command (in `$PATH`) | **Green** (bold) |
| Shell builtin | **Cyan** (bold) |
| AI prefix (`?`, `ai:`) | **Purple** (bold) |
| Explicit path (`./`, `/`) | Yellow |
| Unknown command | Red |
| Flags (`-x`, `--flag`) | Blue |
| Strings (`"..."`, `'...'`) | Yellow |
| Pipes, redirects (`\|`, `>`) | Cyan |
| Variables (`$VAR`) | Green |
| Comments (`# ...`) | Gray (italic) |

## Autosuggestions

As you type, shako shows gray inline suggestions from your command history. Accept with:

- **Right arrow** — accept the full suggestion
- **Ctrl+Right** — accept one word at a time

## Prompt

shako integrates natively with [Starship](https://starship.rs/):

- Exit code, command duration, and terminal width are tracked for Starship modules
- Background job count drives Starship's jobs module
- Left and right prompts are rendered in parallel (two `starship prompt` calls run simultaneously)
- `STARSHIP_SHELL=shako` is set so Starship's shell module shows the correct name

shako creates `~/.config/shako/starship.toml` on first run, merging your global Starship config with shako-specific settings.

If Starship isn't installed, a minimal `❯` prompt is used.

## Fish Compatibility

shako supports fish-style syntax for common operations:

| fish | shako |
|---|---|
| `set -x VAR value` | Works the same |
| `set -gx VAR value` | Works the same |
| `set -e VAR` | Works the same |
| `set` (list all) | Works the same |

### Fish Config Import

Run `fish-import` or use the first-run wizard to import from `~/.config/fish/`:

- **Aliases** — converted to `alias name='value'` format
- **Abbreviations** — preserved as-is
- **Environment variables** — `set -gx` preserved
- **PATH entries** — deduplicated, converted to `fish_add_path`
- **Functions** — converted to `function name() { body }` format

Fish-specific constructs (`bind`, `emit`, `status`, `string`, tool init lines) are stripped. Complex scripts are commented with `# [fish]` prefix.

## Signal Handling

- **Ctrl-C** — sends SIGINT to the foreground process, not the shell
- **Ctrl-\\** — sends SIGQUIT to the foreground process
- **Ctrl-Z** — sends SIGTSTP to the foreground process
- **Ctrl-D** — exits the shell (on empty line)

The shell ignores SIGINT, SIGQUIT, SIGTSTP, SIGTTOU, and SIGTTIN. Children have signals reset to defaults via `pre_exec`.
