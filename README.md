# fnip

Command snippets with variables for your terminal. Store that huge command once, run it with `fnip name`.

Single binary, no runtime dependencies. Works on **Windows**, **Linux** and **macOS**.

Feito por IA.

```bash
fnip -a dnode 'docker run -it --rm -v "$PWD:/app" -w /app node:{{ver:20}} {{@}}'

fnip dnode                  # node:20
fnip dnode ver=18 bash      # node:18, running bash
```

## Installation

Requires [Rust](https://rustup.rs) 1.70 or newer.

```bash
git clone https://github.com/Habbora/fnip
cd fnip
cargo install --path .
```

This builds the `fnip` binary and copies it to `~/.cargo/bin`, which the Rust installer already puts on your `PATH`.

To just build: `cargo build --release` (output in `target/release/fnip`).

`fnip --install` copies that binary onto the user `PATH` and installs shell completion. `fnip --uninstall` removes both. Snippets stay in `snippets.json`.

| OS | Binary | PATH is written to |
|---|---|---|
| Windows | `%LOCALAPPDATA%\fnip\fnip.exe` | the user `Path` registry value |
| Linux | `~/.local/bin/fnip` | `~/.profile` and `~/.bashrc`, or `~/.zprofile` and `~/.zshrc` |
| macOS | `~/.local/bin/fnip` | `~/.zprofile` and `~/.zshrc` (bash: `~/.bash_profile`) |

## Commands

Run `fnip` or `fnip -h` to print this list in the terminal.

| Command | What it does |
|---|---|
| `fnip -h`, `fnip --help` | print help |
| `fnip -a <name> "<command>"` | save or overwrite a snippet |
| `fnip -a <name>` | ask for the command; quotes you type are stored as-is |
| `<text> \| fnip -a <name>` | same, reading the command from stdin |
| `fnip -l`, `fnip --list` | list every snippet |
| `fnip -s <name>`, `fnip --show <name>` | print the raw template |
| `fnip -d <name> [args]`, `fnip --dry` | print the rendered command, do not run it |
| `fnip -e <name> [args]`, `fnip -x`, `fnip --exec` | run a snippet |
| `fnip <name> [args]` | same as `fnip --exec <name> [args]` |
| `fnip -r <name>`, `fnip --remove <name>` | delete a snippet |
| `fnip --install` | copy `fnip` onto `PATH` and install completion |
| `fnip --uninstall` | remove the binary, the `PATH` entry and the completion |
| `fnip --setup` | install shell completion only |
| `fnip --unsetup` | remove shell completion only |
| `fnip --completions <shell>` | print the completion script (`bash`, `zsh` or `powershell`) |

### Save

A short command can be the next argument. In bash and zsh, use single quotes so the shell does not expand `$VAR` before `fnip` sees it.

```bash
fnip -a gp 'git add -A && git commit -m "{{msg}}" && git push'
```

`fnip -a <name>` with no command prompts for one line. `'` and `"` are stored as typed:

```text
fnip -a bunhash
command for bunhash: bun -e 'console.log(await Bun.password.hash("{{senha}}", { algorithm: "argon2id" }))'
saved: bunhash
```

A pipe does the same when you are not at a prompt. PowerShell:

```powershell
@'
bun -e 'console.log(await Bun.password.hash("{{senha}}", { algorithm: "argon2id" }))'
'@ | fnip -a bunhash
```

bash and zsh:

```bash
fnip -a bunhash <<'EOF'
bun -e 'console.log(await Bun.password.hash("{{senha}}", { algorithm: "argon2id" }))'
EOF
```

### Look, run, delete

```bash
fnip -l
fnip -s gp
fnip -d gp msg="fix login"
# git add -A && git commit -m "fix login" && git push

fnip gp msg="fix login"          # runs it
fnip --exec gp msg="fix login"   # same thing
fnip -r gp
```

### Install and completion

```bash
fnip --install     # binary on PATH + completion
fnip --uninstall   # removes both; snippets stay

fnip --setup       # completion only
fnip --unsetup     # remove completion only

fnip --completions bash
fnip --completions zsh
fnip --completions powershell
```

## Templates

| Syntax | Meaning |
|---|---|
| `{{var}}` | required variable; errors out if no value is given |
| `{{var:default}}` | variable with a default value |
| `{{@}}` | extra arguments (the ones that aren't `key=value`) |

At run time, arguments in the form `key=value` fill the variables. Everything else becomes `{{@}}`. If the template has no `{{@}}` and there are leftovers, they are appended to the end of the command. Use `--` to force everything after it to be treated as extra.

```bash
fnip -a dk 'docker run -it --rm'
fnip dk alpine sh          # appended, because the template has no {{@}}
fnip dk -- alpine sh       # '--' forces the rest to be extra
```

## Tab completion

On the **first run** in an interactive terminal, `fnip` asks whether you want to enable completion. Answer `Y` and it writes a line to your shell's startup file. **Open a new terminal** and Tab works for flags, snippet names and template variables (`ver=`).

`--setup` writes one block between `# >>> fnip >>>` and `# <<< fnip <<<`. Running it again replaces that block instead of appending a second copy:

| Shell | File |
|---|---|
| bash | `~/.bashrc` (macOS: `~/.bash_profile`) |
| zsh | `~/.zshrc` |
| PowerShell 7 and 5.1 | each one's `$PROFILE` (`fnip` asks PowerShell itself where it is) |

```
# >>> fnip >>>
command -v fnip >/dev/null 2>&1 && eval "$(fnip --completions bash)"
# <<< fnip <<<
```

Requirements: the `fnip` executable must be on your `PATH`. On Windows, if PowerShell complains about the execution policy: `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`.

Manual install, if you prefer: `eval "$(fnip --completions bash|zsh)"` in your profile, or `fnip --completions powershell | Out-String | Invoke-Expression` in your `$PROFILE`.

Names come from `snippets.json` on every Tab, so new snippets show up immediately.

## How it works

```
fnip dnode ver=18 bash
 │
 ├─ reads snippets.json (the OS config folder)
 ├─ finds the template for "dnode"
 ├─ substitutes {{variables}} (done by fnip, not by the shell)
 └─ runs the final string in a shell
      Linux/macOS: sh -c "<string>"
      Windows:     pwsh -Command "<string>"   (falls back to powershell.exe if pwsh is missing)
```

Snippets live in a JSON file that you can edit by hand or put under version control:

| OS | Path |
|---|---|
| Linux | `~/.config/fnip/snippets.json` |
| macOS | `~/Library/Application Support/fnip/snippets.json` |
| Windows | `%AppData%\fnip\snippets.json` |

The child process inherits `stdin`, `stdout` and `stderr`, so interactive commands (`docker run -it`, `ssh`) work, and the command's exit code is propagated to `fnip`.

## Known limitations

- **Syntax depends on the shell.** The template is text interpreted by `sh` or PowerShell. A snippet with `$(...)` or `export` written for bash won't run in PowerShell. Prefer simple commands (`git`, `docker`, `npm`) if you share snippets across systems.
- **`&&` on Windows** only works in PowerShell 7+ (`pwsh`), not in `powershell.exe` 5.1.
- **No value escaping.** A variable's value is pasted as text. `fnip gp msg='x"; rm -rf ~; "'` would be interpreted by the shell. Only use values you trust.
- If the child process is killed by a signal (Unix), `fnip` returns exit code 1.

## Roadmap

- [ ] `--edit`: open a snippet in `$EDITOR`
- [ ] Interactively prompt for missing variables
- [ ] Per-snippet configurable shell
- [ ] Snippets that point to a script file
- [x] Completion for bash, zsh and PowerShell
- [x] Automatic completion setup
- [x] `--install` / `--uninstall` on Windows, Linux and macOS
- [ ] Completion for fish
- [ ] Releases with binaries for all 3 platforms (GitHub Actions)

## License

MIT. See [LICENSE](LICENSE).
