// fnip - command snippet manager (Windows, Linux, macOS)
//
//   fnip -a, --add <name> "<command with {{variables}}>"
//   fnip -r, --remove <name>
//   fnip -l, --list
//   fnip -s, --show <name>               raw template
//   fnip -d, --dry <name> [args]         render without running
//   fnip -e, -x, --exec <name> [args]    run (or just: fnip <name> [args])
//   fnip --setup | --unsetup             install/remove shell completion
//   fnip --install | --uninstall         copy fnip onto PATH (+ completion) / remove it
//   fnip --completions <shell>           print the completion script
//
// Template: {{var}}  {{var:default}}  {{@}} (extra args)
// Args: key=value fills variables; the rest becomes {{@}}. "--" forces "extra".

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, ErrorKind, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::OnceLock;

use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Snippet {
    command: String,
}

// BTreeMap keeps keys sorted: --list comes out alphabetical without manual sorting
type Store = BTreeMap<String, Snippet>;

// ---------- persistence ----------

fn store_path() -> Result<PathBuf, String> {
    // Windows: %AppData%\fnip | Linux: ~/.config/fnip | macOS: ~/Library/Application Support/fnip
    let dir = dirs::config_dir().ok_or("could not find the config directory")?;
    Ok(dir.join("fnip").join("snippets.json"))
}

fn load() -> Result<Store, String> {
    let p = store_path()?;
    match fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| format!("{} is corrupted: {e}", p.display())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Store::new()), // first run
        Err(e) => Err(e.to_string()),
    }
}

fn save(store: &Store) -> Result<(), String> {
    let p = store_path()?;
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    let data = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    // Write the new JSON aside, then swap it into place. A crash during the
    // write leaves the previous snippets.json untouched.
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, data).map_err(|e| e.to_string())?;
    replace_file(&tmp, &p)
}

// std::fs::rename does not replace an existing destination on Windows.
fn replace_file(from: &Path, to: &Path) -> Result<(), String> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) if cfg!(windows) && to.exists() => {
            fs::remove_file(to).map_err(|e| e.to_string())?;
            fs::rename(from, to).map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

// ---------- template ----------

// OnceLock: compiles the regex once, on first use
fn placeholder() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"\{\{\s*(@|[A-Za-z_][A-Za-z0-9_-]*)\s*(?::([^}]*))?\}\}").unwrap()
    })
}

fn kv() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?s)^([A-Za-z_][A-Za-z0-9_-]*)=(.*)$").unwrap())
}

fn parse_args(args: &[String]) -> (HashMap<String, String>, Vec<String>) {
    let mut vars = HashMap::new();
    let mut extras = Vec::new();
    let mut force_extra = false;

    for a in args {
        if force_extra {
            extras.push(a.clone());
            continue;
        }
        if a == "--" {
            force_extra = true;
            continue;
        }
        match kv().captures(a) {
            Some(c) => {
                vars.insert(c[1].to_string(), c[2].to_string());
            }
            None => extras.push(a.clone()),
        }
    }
    (vars, extras)
}

fn render(tpl: &str, vars: &HashMap<String, String>, extras: &[String]) -> Result<String, String> {
    let extra_str = extras.join(" ");
    let mut used_extras = false;
    let mut missing: Vec<String> = Vec::new();

    let mut out = placeholder()
        .replace_all(tpl, |c: &Captures| {
            let name = &c[1];
            if name == "@" {
                used_extras = true;
                return extra_str.clone();
            }
            if let Some(v) = vars.get(name) {
                return v.clone();
            }
            // group 2 exists only if there was a ":default" (even an empty one)
            if let Some(d) = c.get(2) {
                return d.as_str().to_string();
            }
            missing.push(name.to_string());
            c[0].to_string()
        })
        .into_owned();

    if !missing.is_empty() {
        return Err(format!(
            "missing variables: {} (pass them as name=value)",
            missing.join(", ")
        ));
    }
    // template without {{@}} but with leftovers: append them (handy for "fnip dnode bash")
    if !used_extras && !extra_str.is_empty() {
        out.push(' ');
        out.push_str(&extra_str);
    }
    Ok(out)
}

// ---------- execution ----------

fn run(line: &str) -> i32 {
    // (program, args that precede the command string), in order of preference
    let attempts: Vec<(&str, Vec<&str>)> = if cfg!(windows) {
        vec![
            ("pwsh", vec!["-NoLogo", "-NoProfile", "-Command"]),
            ("powershell", vec!["-NoLogo", "-NoProfile", "-Command"]),
        ]
    } else {
        vec![("sh", vec!["-c"])]
    };

    for (prog, pre) in attempts {
        // .status() inherits stdin/stdout/stderr: interactive commands (docker -it) work
        match Command::new(prog).args(&pre).arg(line).status() {
            Ok(st) => return st.code().unwrap_or(1), // None = killed by a signal (Unix)
            Err(e) if e.kind() == ErrorKind::NotFound => continue, // try the next shell
            Err(e) => {
                eprintln!("fnip: failed to run {prog}: {e}");
                return 127;
            }
        }
    }
    eprintln!("fnip: no shell found");
    127
}

// ---------- completion ----------

// include_str! reads the file at compile time and embeds it in the binary:
// the end user only needs the executable, no loose files.
const COMPLETION_BASH: &str = include_str!("../completions/fnip.bash");
const COMPLETION_ZSH: &str = include_str!("../completions/fnip.zsh");
const COMPLETION_PWSH: &str = include_str!("../completions/fnip.ps1");

/// Variable names in a template, deduplicated, in order of appearance.
fn template_vars(tpl: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in placeholder().captures_iter(tpl) {
        let name = c[1].to_string();
        if name != "@" && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

fn print_completions(rest: &[String]) -> Result<i32, String> {
    let script = match rest.first().map(String::as_str) {
        Some("bash") => COMPLETION_BASH,
        Some("zsh") => COMPLETION_ZSH,
        Some("powershell") | Some("pwsh") => COMPLETION_PWSH,
        _ => return Err("usage: fnip --completions <bash|zsh|powershell>".into()),
    };
    print!("{script}");
    Ok(0)
}

// ---------- automatic setup ----------
//
// A child process CANNOT modify the shell that started it. So what we do is write a line
// to the shell's startup file (profile): the next terminal you open starts with completion.

const MARK_BEGIN: &str = "# >>> fnip >>>";
const MARK_END: &str = "# <<< fnip <<<";

struct Target {
    shell: &'static str,
    profile: PathBuf,
    block: String,
}

/// Asks PowerShell itself where its $PROFILE lives. More reliable than building the
/// path by hand (OneDrive often redirects "Documents").
fn powershell_profile(exe: &str) -> Option<PathBuf> {
    let out = Command::new(exe)
        .args(["-NoLogo", "-NoProfile", "-Command", "$PROFILE"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

fn setup_targets() -> Vec<Target> {
    let mut v = Vec::new();

    if cfg!(windows) {
        let block = format!(
            "{MARK_BEGIN}\nif (Get-Command fnip -ErrorAction SilentlyContinue) {{ fnip --completions powershell | Out-String | Invoke-Expression }}\n{MARK_END}\n"
        );
        // PowerShell 7 (pwsh) and Windows PowerShell 5.1 have separate profiles
        for (shell, exe) in [("pwsh", "pwsh"), ("powershell", "powershell")] {
            if let Some(profile) = powershell_profile(exe) {
                v.push(Target { shell, profile, block: block.clone() });
            }
        }
    } else if let Some(home) = dirs::home_dir() {
        let current = std::env::var("SHELL").unwrap_or_default();
        // "command -v fnip" avoids startup errors if fnip is removed from PATH;
        // eval "$(...)" works even on macOS's bash 3.2, unlike source <(...)
        let block = |sh: &str| {
            format!(
                "{MARK_BEGIN}\ncommand -v fnip >/dev/null 2>&1 && eval \"$(fnip --completions {sh})\"\n{MARK_END}\n"
            )
        };

        // macOS opens login shells, which read .bash_profile instead of .bashrc
        let bash_rc = if cfg!(target_os = "macos") {
            home.join(".bash_profile")
        } else {
            home.join(".bashrc")
        };
        if current.ends_with("bash") || bash_rc.exists() {
            v.push(Target { shell: "bash", profile: bash_rc, block: block("bash") });
        }

        let zsh_rc = home.join(".zshrc");
        if current.ends_with("zsh") || zsh_rc.exists() {
            v.push(Target { shell: "zsh", profile: zsh_rc, block: block("zsh") });
        }
    }
    v
}

fn block_is_current(file: &str, block: &str) -> bool {
    let Some(start) = file.find(MARK_BEGIN) else { return false };
    let Some(end_rel) = file[start..].find(MARK_END) else { return false };
    let end = start + end_rel + MARK_END.len();
    file[start..end].trim() == block.trim()
}

/// Ok(true) = written, Ok(false) = already current. A stale block is replaced, not duplicated.
fn install_target(t: &Target) -> Result<bool, String> {
    let mut existing = match fs::read_to_string(&t.profile) {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", t.profile.display())),
    };
    if existing.contains(MARK_BEGIN) {
        if block_is_current(&existing, &t.block) {
            return Ok(false);
        }
        remove_marked(&t.profile, MARK_BEGIN, MARK_END)?;
        existing = fs::read_to_string(&t.profile).unwrap_or_default();
    }
    if let Some(dir) = t.profile.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&t.profile)
        .map_err(|e| format!("{}: {e}", t.profile.display()))?;
    let sep = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    write!(file, "{sep}\n{}", t.block).map_err(|e| e.to_string())?;
    Ok(true)
}

fn remove_block(path: &Path) -> Result<bool, String> {
    remove_marked(path, MARK_BEGIN, MARK_END)
}

fn remove_marked(path: &Path, begin: &str, end_mark: &str) -> Result<bool, String> {
    let s = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let Some(start) = s.find(begin) else { return Ok(false) };
    let Some(end_rel) = s[start..].find(end_mark) else { return Ok(false) };
    let mut end = start + end_rel + end_mark.len();
    if s[end..].starts_with("\r\n") {
        end += 2;
    } else if s[end..].starts_with('\n') {
        end += 1;
    }
    let head = s[..start].trim_end();
    let tail = &s[end..];
    let new = if head.is_empty() { tail.to_string() } else { format!("{head}\n{tail}") };
    fs::write(path, new).map_err(|e| e.to_string())?;
    Ok(true)
}

fn exe_in_path() -> bool {
    let name = binary_name();
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
        .unwrap_or(false)
}

fn setup_marker() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("fnip").join("setup-done"))
}

fn mark_setup_done() {
    if let Some(p) = setup_marker() {
        if let Some(d) = p.parent() {
            let _ = fs::create_dir_all(d);
        }
        let _ = fs::write(p, "1");
    }
}

fn run_setup() -> Result<i32, String> {
    let targets = setup_targets();
    if targets.is_empty() {
        mark_setup_done();
        return Err("no supported shell found (bash, zsh, PowerShell)".into());
    }
    for t in &targets {
        match install_target(t) {
            Ok(true) => println!("ok: {} -> configured ({})", t.shell, t.profile.display()),
            Ok(false) => println!("ok: {} -> already configured ({})", t.shell, t.profile.display()),
            Err(e) => eprintln!("error: {}: {e}", t.shell),
        }
    }
    if !exe_in_path() {
        println!("\nwarning: the 'fnip' executable is not in your PATH. Move it to a PATH folder");
        println!("(e.g. ~/.local/bin or ~/.cargo/bin), otherwise Tab won't find the command.");
    }
    println!("\nOpen a new terminal to activate completion.");
    if cfg!(windows) {
        println!("If PowerShell complains about the execution policy:");
        println!("  Set-ExecutionPolicy -Scope CurrentUser RemoteSigned");
    }
    mark_setup_done();
    Ok(0)
}

fn run_unsetup() -> Result<i32, String> {
    for t in setup_targets() {
        match remove_block(&t.profile) {
            Ok(true) => println!("ok: {} -> removed from {}", t.shell, t.profile.display()),
            Ok(false) => println!("--: {} -> nothing to remove", t.shell),
            Err(e) => eprintln!("error: {}: {e}", t.shell),
        }
    }
    Ok(0)
}

/// On the first interactive run, offers to install completion. Asks only once.
fn maybe_first_run() {
    let Some(marker) = setup_marker() else { return };
    // no terminal (pipe, script, the completion script itself): never ask
    if marker.exists() || !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return;
    }
    print!("First run. Enable tab completion in your shell? [Y/n] ");
    let _ = io::stdout().flush();
    let mut ans = String::new();
    if io::stdin().read_line(&mut ans).is_err() {
        return;
    }
    match ans.trim().to_lowercase().as_str() {
        "" | "y" | "yes" => {
            if let Err(e) = run_setup() {
                eprintln!("fnip: {e}");
            }
            println!();
        }
        _ => {
            mark_setup_done();
            println!("Ok. Run `fnip --setup` whenever you want.\n");
        }
    }
}

// ---------- install / uninstall ----------
//
// One binary for every OS. A shell script would be install.ps1 plus install.sh.
// --install copies this executable onto a user directory and puts that directory
// on the user PATH, then reuses --setup for completion. Snippets are not touched.

const PATH_MARK_BEGIN: &str = "# >>> fnip path >>>";
const PATH_MARK_END: &str = "# <<< fnip path <<<";

fn binary_name() -> &'static str {
    if cfg!(windows) { "fnip.exe" } else { "fnip" }
}

fn install_dir() -> Result<PathBuf, String> {
    if cfg!(windows) {
        let base = dirs::data_local_dir().ok_or("could not find the local data directory")?;
        Ok(base.join("fnip"))
    } else {
        let home = dirs::home_dir().ok_or("could not find the home directory")?;
        Ok(home.join(".local").join("bin"))
    }
}

fn installed_path_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("fnip").join("installed-path"))
}

fn same_path(a: &Path, b: &Path) -> bool {
    let ca = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let cb = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    if cfg!(windows) {
        ca.to_string_lossy().eq_ignore_ascii_case(&cb.to_string_lossy())
    } else {
        ca == cb
    }
}

fn install_binary(dest: &Path) -> Result<(), String> {
    let src = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if dest.exists() && same_path(&src, dest) {
        return Ok(());
    }
    // Stage beside the destination, then move it into place.
    // Linux refuses to truncate a running binary (ETXTBSY); rename swaps the inode.
    // Windows rename does not replace an existing file, so the old image is moved aside.
    let tmp = dest.with_extension("new");
    fs::copy(&src, &tmp).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&tmp).map_err(|e| e.to_string())?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tmp, perms).map_err(|e| e.to_string())?;
    }
    if !cfg!(windows) || !dest.exists() {
        return fs::rename(&tmp, dest).map_err(|e| e.to_string());
    }
    let old = dest.with_extension("old");
    let _ = fs::remove_file(&old);
    if let Err(e) = fs::rename(dest, &old) {
        let _ = fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    if let Err(e) = fs::rename(&tmp, dest) {
        let _ = fs::rename(&old, dest);
        return Err(e.to_string());
    }
    let _ = fs::remove_file(&old);
    Ok(())
}

fn prepend_process_path(dir: &Path) {
    let old = std::env::var_os("PATH").unwrap_or_default();
    let mut parts = vec![dir.to_path_buf()];
    parts.extend(std::env::split_paths(&old));
    if let Ok(joined) = std::env::join_paths(parts) {
        std::env::set_var("PATH", joined);
    }
}

fn edit_user_path(dir: &Path, add: bool) -> Result<(), String> {
    if cfg!(windows) {
        // Read and write the stored user PATH (unexpanded), not this process's PATH.
        // setx would truncate at 1024 characters.
        let script = r#"
$d = $env:FNIP_BIN
$add = $env:FNIP_PATH_ADD -eq '1'
$p = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($null -eq $p) { $p = '' }
function Norm([string]$s) {
  if ([string]::IsNullOrWhiteSpace($s)) { return '' }
  return ([Environment]::ExpandEnvironmentVariables($s).Trim().TrimEnd('\','/')).ToLowerInvariant()
}
$target = Norm $d
$parts = @($p -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) -and (Norm $_) -ne $target })
if ($add) { $parts += $d }
[Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User')
"#;
        let status = Command::new("powershell")
            .args(["-NoLogo", "-NoProfile", "-Command", script])
            .env("FNIP_BIN", dir)
            .env("FNIP_PATH_ADD", if add { "1" } else { "0" })
            .status()
            .map_err(|e| format!("powershell: {e}"))?;
        if !status.success() {
            return Err("failed to update the user PATH".into());
        }
        return Ok(());
    }

    let Some(home) = dirs::home_dir() else {
        return Err("could not find the home directory".into());
    };
    // $HOME/.local/bin matches install_dir() on Linux and macOS.
    // Login shells and interactive shells do not read the same file:
    //   Linux bash: ~/.profile + ~/.bashrc
    //   Linux zsh:  ~/.zprofile + ~/.zshrc
    //   macOS zsh:  ~/.zprofile + ~/.zshrc
    //   macOS bash: ~/.bash_profile
    let block = format!("{PATH_MARK_BEGIN}\nexport PATH=\"$HOME/.local/bin:$PATH\"\n{PATH_MARK_END}\n");
    let files = unix_path_files(&home);
    for path in files {
        if add {
            let existing = match fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
                Err(e) => return Err(format!("{}: {e}", path.display())),
            };
            if existing.contains(PATH_MARK_BEGIN) {
                continue;
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let sep = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
            write!(file, "{sep}\n{block}").map_err(|e| e.to_string())?;
        } else {
            remove_marked(&path, PATH_MARK_BEGIN, PATH_MARK_END)?;
        }
    }
    Ok(())
}

/// Startup files that actually apply `$HOME/.local/bin` on the next shell.
fn unix_path_files(home: &Path) -> Vec<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_default();
    let bash = shell.ends_with("bash")
        || home.join(".bashrc").exists()
        || home.join(".bash_profile").exists();
    let zsh = shell.ends_with("zsh")
        || home.join(".zshrc").exists()
        || home.join(".zprofile").exists();

    let mut files = Vec::new();
    if cfg!(target_os = "macos") {
        if bash {
            files.push(home.join(".bash_profile"));
        }
        // macOS defaults to zsh. Terminal.app is a login shell, so .zprofile counts.
        if zsh || !bash {
            files.push(home.join(".zprofile"));
            files.push(home.join(".zshrc"));
        }
    } else if bash || !zsh {
        files.push(home.join(".profile"));
        files.push(home.join(".bashrc"));
        if zsh {
            files.push(home.join(".zprofile"));
            files.push(home.join(".zshrc"));
        }
    } else {
        files.push(home.join(".zprofile"));
        files.push(home.join(".zshrc"));
    }
    files
}

fn config_base() -> Option<PathBuf> {
    dirs::config_dir()
}

/// The command used to be `f` and the config directory `fcmd`.
fn migrate_legacy_config() {
    let Some(base) = config_base() else { return };
    let old_dir = base.join("fcmd");
    let new_dir = base.join("fnip");
    if !old_dir.is_dir() {
        return;
    }
    let _ = fs::create_dir_all(&new_dir);
    for name in ["snippets.json", "setup-done", "installed-path"] {
        let from = old_dir.join(name);
        let to = new_dir.join(name);
        if from.is_file() && !to.exists() {
            let _ = fs::rename(&from, &to);
        }
    }
    let _ = fs::remove_dir(&old_dir);
}

fn retire_previous_binary(new_dest: &Path) {
    let Some(old) = recorded_install() else { return };
    if !old.is_file() || same_path(&old, new_dest) {
        return;
    }
    let Some(name) = old.file_name().and_then(|s| s.to_str()) else { return };
    if name == "f" || name == "f.exe" {
        if let Err(e) = fs::remove_file(&old) {
            eprintln!("fnip: could not remove previous binary {}: {e}", old.display());
        }
    }
}

fn record_install(path: &Path) -> Result<(), String> {
    let file = installed_path_file().ok_or("could not find the config directory")?;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&file, path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())
}

fn recorded_install() -> Option<PathBuf> {
    let file = installed_path_file()?;
    let s = fs::read_to_string(file).ok()?;
    let s = s.trim();
    if s.is_empty() { None } else { Some(PathBuf::from(s)) }
}

fn schedule_delete(path: &Path) -> Result<(), String> {
    let parent = path.parent().unwrap_or(path);
    let script = format!(
        "ping 127.0.0.1 -n 3 >nul & del /f /q \"{}\" & rmdir \"{}\"",
        path.display(),
        parent.display()
    );
    Command::new("cmd")
        .args(["/C", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn remove_installed_binary(path: &Path) -> Result<(), String> {
    if !path.exists() {
        println!("--: {} is already gone", path.display());
        return Ok(());
    }
    let running = std::env::current_exe().ok();
    let same = running.as_ref().map(|p| same_path(p, path)).unwrap_or(false);
    if same && cfg!(windows) {
        schedule_delete(path)?;
        println!("ok: {} will be removed when this process exits", path.display());
        return Ok(());
    }
    fs::remove_file(path).map_err(|e| e.to_string())?;
    if cfg!(windows) {
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
    println!("ok: removed {}", path.display());
    Ok(())
}

fn run_install() -> Result<i32, String> {
    let dest = install_dir()?.join(binary_name());
    retire_previous_binary(&dest);
    install_binary(&dest)?;
    record_install(&dest)?;
    let dir = dest.parent().unwrap().to_path_buf();
    edit_user_path(&dir, true)?;
    prepend_process_path(&dir);
    println!("installed: {}", dest.display());
    run_setup()?;
    Ok(0)
}

fn run_uninstall() -> Result<i32, String> {
    let dest = recorded_install().unwrap_or(install_dir()?.join(binary_name()));
    if let Err(e) = run_unsetup() {
        eprintln!("fnip: {e}");
    }
    if let Some(dir) = dest.parent() {
        if let Err(e) = edit_user_path(dir, false) {
            eprintln!("fnip: {e}");
        }
    }
    remove_installed_binary(&dest)?;
    if let Some(p) = installed_path_file() {
        let _ = fs::remove_file(p);
    }
    if let Some(p) = setup_marker() {
        let _ = fs::remove_file(p);
    }
    println!("\nSnippets were kept. Open a new terminal.");
    Ok(0)
}

// ---------- CLI ----------

fn usage() {
    println!(
        r#"fnip - command snippets

  fnip -a, --add <name> "<command>"   create/overwrite
  fnip -r, --remove <name>            remove
  fnip -l, --list                     list all
  fnip -s, --show <name>              show the raw template
  fnip -d, --dry <name> [args]        render without running
  fnip -e, -x, --exec <name> [args]   run (or just: fnip <name> [args])
  fnip --install                      copy fnip onto PATH and install completion
  fnip --uninstall                    remove fnip from PATH and remove completion
  fnip --setup                        install shell completion
  fnip --unsetup                      remove shell completion
  fnip --completions <shell>          print the script (bash|zsh|powershell)

Template: {{{{var}}}}  {{{{var:default}}}}  {{{{@}}}}
Args:     key=value fills variables; the rest goes into {{{{@}}}}"#
    );
}

fn exec_snippet(store: &Store, name: &str, args: &[String], dry: bool) -> Result<i32, String> {
    let sn = store
        .get(name)
        .ok_or_else(|| format!("no such snippet: {name} (use --list)"))?;
    let (vars, extras) = parse_args(args);
    let line = render(&sn.command, &vars, &extras)?;
    if dry {
        println!("{line}");
        return Ok(0);
    }
    Ok(run(&line))
}

fn real_main() -> Result<i32, String> {
    migrate_legacy_config();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(action) = args.first().map(String::as_str) else {
        usage();
        return Ok(0);
    };
    let rest = &args[1..];

    // these don't depend on snippets.json: they work even if the file is corrupted
    match action {
        "--completions" => return print_completions(rest),
        "--setup" => return run_setup(),
        "--unsetup" => return run_unsetup(),
        "--install" => return run_install(),
        "--uninstall" => return run_uninstall(),
        _ => {}
    }
    // asks once, and only when there is an interactive terminal
    if !action.starts_with("--complete") {
        maybe_first_run();
    }

    let mut store = load()?;

    match action {
        "-h" | "--help" => {
            usage();
            Ok(0)
        }
        // hidden flags, called by the completion scripts on every Tab.
        // They start with "-", so they never collide with a snippet name.
        "--complete-names" => {
            for name in store.keys() {
                println!("{name}");
            }
            Ok(0)
        }
        "--complete-vars" => {
            if let Some(sn) = rest.first().and_then(|n| store.get(n)) {
                for v in template_vars(&sn.command) {
                    println!("{v}=");
                }
            }
            Ok(0)
        }
        "-a" | "--add" => {
            if rest.len() < 2 {
                return Err(r#"usage: fnip --add <name> "<command>""#.into());
            }
            let name = &rest[0];
            if name.starts_with('-') {
                return Err("name cannot start with '-'".into());
            }
            store.insert(
                name.clone(),
                Snippet {
                    command: rest[1..].join(" "),
                },
            );
            save(&store)?;
            println!("saved: {name}");
            Ok(0)
        }
        "-r" | "--remove" => {
            if rest.len() != 1 {
                return Err("usage: fnip --remove <name>".into());
            }
            if store.remove(&rest[0]).is_none() {
                return Err(format!("no such snippet: {}", rest[0]));
            }
            save(&store)?;
            println!("removed: {}", rest[0]);
            Ok(0)
        }
        "-l" | "--list" => {
            for (name, sn) in &store {
                println!("{:<16} {}", name, sn.command);
            }
            Ok(0)
        }
        "-s" | "--show" => {
            if rest.len() != 1 {
                return Err("usage: fnip --show <name>".into());
            }
            let sn = store
                .get(&rest[0])
                .ok_or_else(|| format!("no such snippet: {}", rest[0]))?;
            println!("{}", sn.command);
            Ok(0)
        }
        "-d" | "--dry" | "-e" | "-x" | "--exec" => {
            if rest.is_empty() {
                return Err(format!("usage: fnip {action} <name> [args]"));
            }
            let dry = action == "-d" || action == "--dry";
            exec_snippet(&store, &rest[0], &rest[1..], dry)
        }
        _ if action.starts_with('-') => Err(format!("unknown flag: {action} (see --help)")),
        // shortcut: "fnip name args" == "fnip --exec name args"
        _ => exec_snippet(&store, action, rest, false),
    }
}

fn main() -> ExitCode {
    match real_main() {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(e) => {
            eprintln!("fnip: {e}");
            ExitCode::from(1)
        }
    }
}
