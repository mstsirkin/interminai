//! Exercise Git through the public PTY interface, and verify the actual index.
mod common;

use assert_cmd::Command;
use common::{emulator_args, interminai_client_bin, interminai_server_bin};
use std::fs;
use std::time::{Duration, Instant};
use tempfile::TempDir;

struct Repo {
    dir: TempDir,
    original: String,
}

impl Repo {
    fn new() -> Self {
        let repo = Self {
            dir: TempDir::new().unwrap(),
            original: (1..=40).map(|n| format!("line {n:02}\n")).collect(),
        };
        repo.git(&["init", "-q"]);
        repo.git(&["config", "user.name", "Interminai test"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        fs::write(repo.dir.path().join("tracked.txt"), &repo.original).unwrap();
        repo.git(&["add", "tracked.txt"]);
        repo.git(&["commit", "-qm", "fixture"]);
        repo
    }

    fn command(&self, bin: &str) -> Command {
        let mut cmd = Command::new(bin);
        cmd.current_dir(self.dir.path())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", self.dir.path())
            .env("LC_ALL", "C")
            .env("TERM", "xterm-256color")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_EDITOR", "vim -Nu NONE -n -i NONE")
            .env("PS1", "READY> ")
            .timeout(Duration::from_secs(10));
        cmd
    }

    fn git(&self, args: &[&str]) -> String {
        let output = self
            .command("git")
            .args(args)
            .assert()
            .success()
            .get_output()
            .clone();
        String::from_utf8(output.stdout).unwrap()
    }

    fn change(&self, changes: &[(usize, &str)]) -> String {
        let mut content = self.original.clone();
        for (line, replacement) in changes {
            content = content.replace(&format!("line {line:02}"), replacement);
        }
        fs::write(self.dir.path().join("tracked.txt"), &content).unwrap();
        content
    }

    fn three_hunks(&self) -> String {
        self.change(&[
            (3, "FIRST change"),
            (20, "SECOND change"),
            (37, "THIRD change"),
        ])
    }

    fn start(&self, args: &[&str]) -> Session<'_> {
        let output = self
            .command(&interminai_server_bin())
            .arg("start")
            .args(emulator_args())
            .args(["--scrollback", "200", "--"])
            .args(args)
            .assert()
            .success()
            .get_output()
            .clone();
        let stdout = String::from_utf8(output.stdout).unwrap();
        let socket = stdout
            .lines()
            .find_map(|line| line.strip_prefix("Socket: "))
            .expect("start should report a socket")
            .trim()
            .to_string();
        Session { repo: self, socket }
    }

    fn patch(&self, single_key: bool) -> Session<'_> {
        self.start(&[
            "git",
            "-c",
            if single_key {
                "interactive.singleKey=true"
            } else {
                "interactive.singleKey=false"
            },
            "add",
            "-p",
        ])
    }

    fn assert_index(&self, changes: &[(usize, &str)]) {
        let mut expected = self.original.clone();
        for (line, replacement) in changes {
            expected = expected.replace(&format!("line {line:02}"), replacement);
        }
        assert_eq!(self.git(&["show", ":tracked.txt"]), expected);
    }
}

struct Session<'a> {
    repo: &'a Repo,
    socket: String,
}

impl Session<'_> {
    fn call(&self, action: &str, args: &[&str]) -> String {
        let output = self
            .repo
            .command(&interminai_client_bin())
            .args([action, "--socket", &self.socket])
            .args(args)
            .assert()
            .success()
            .get_output()
            .clone();
        String::from_utf8(output.stdout).unwrap()
    }

    fn input(&self, text: &str) {
        self.call("input", &["--text", text]);
    }

    fn screen(&self) -> String {
        // Plain text is used for assertions; colored rendering is checked separately.
        self.call("output", &["--no-color"])
    }

    fn expect(&self, text: &str, last_line: bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let screen = self.screen();
            let visible = if last_line {
                screen
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("")
            } else {
                &screen
            };
            if visible.contains(text) {
                return screen;
            }
            assert!(Instant::now() < deadline, "Missing {text:?}: {screen}");
            // Block for PTY activity instead of repeatedly sleeping and polling output.
            let status = self.call("wait", &[]);
            assert!(
                !status.contains("Exited: true"),
                "Exited before {text:?}: {}",
                self.screen()
            );
        }
    }

    fn prompt(&self, text: &str) -> String {
        self.expect(text, true)
    }

    fn finish(&self) {
        assert_eq!(self.call("wait", &["--quiet"]).trim(), "0");
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        let _ = self
            .repo
            .command(&interminai_client_bin())
            .args(["stop", "--socket", &self.socket])
            .output();
    }
}

#[test]
fn line_mode_needs_enter_and_index_is_written_after_selection() {
    let repo = Repo::new();
    let worktree = repo.three_hunks();
    let session = repo.patch(false);
    session.prompt("(1/3) Stage this hunk");
    session.input("y");
    session.prompt("]? y"); // Echo proves that the byte reached the PTY.
    assert!(repo.git(&["diff", "--cached"]).is_empty());
    session.input("\\r");
    session.prompt("(2/3) Stage this hunk");
    assert!(repo.git(&["diff", "--cached"]).is_empty());
    session.input("q\\r");
    session.finish();
    repo.assert_index(&[(3, "FIRST change")]);
    assert_eq!(
        fs::read_to_string(repo.dir.path().join("tracked.txt")).unwrap(),
        worktree
    );
}

#[test]
fn single_key_y_n_y_without_enter() {
    let repo = Repo::new();
    repo.three_hunks();
    let session = repo.patch(true);
    session.prompt("(1/3) Stage this hunk");
    session.input("y");
    session.prompt("(2/3) Stage this hunk");
    assert!(repo.git(&["diff", "--cached"]).is_empty());
    session.input("n");
    session.prompt("(3/3) Stage this hunk");
    session.input("y");
    session.finish();
    repo.assert_index(&[(3, "FIRST change"), (37, "THIRD change")]);
}

#[test]
fn all_and_skip_rest_across_files() {
    let repo = Repo::new();
    fs::write(repo.dir.path().join("other.txt"), &repo.original).unwrap();
    repo.git(&["add", "other.txt"]);
    repo.git(&["commit", "-qm", "second file"]);
    fs::write(
        repo.dir.path().join("other.txt"),
        repo.original.replace("line 03", "OTHER change"),
    )
    .unwrap();
    let worktree = repo.three_hunks();
    let session = repo.patch(false);
    session.prompt("Stage this hunk");
    assert!(session.screen().contains("OTHER change"));
    session.input("d\\r");
    session.prompt("(1/3) Stage this hunk");
    session.input("a\\r");
    session.finish();
    assert_eq!(repo.git(&["show", ":tracked.txt"]), worktree);
    assert_eq!(repo.git(&["show", ":other.txt"]), repo.original);
}

#[test]
fn split_hunk_and_select_one_part() {
    let repo = Repo::new();
    repo.change(&[(3, "FIRST change"), (7, "SECOND change")]);
    let session = repo.patch(false);
    session.prompt("Stage this hunk");
    session.input("s\\r");
    session.prompt("(1/2) Stage this hunk");
    session.input("y\\r");
    session.prompt("(2/2) Stage this hunk");
    session.input("n\\r");
    session.finish();
    repo.assert_index(&[(3, "FIRST change")]);
}

#[test]
fn help_navigation_and_search() {
    let repo = Repo::new();
    repo.three_hunks();
    let session = repo.patch(false);
    session.prompt("(1/3) Stage this hunk");
    session.input("?\\r");
    session.expect("stage this hunk", false);
    session.input("j\\r");
    session.prompt("(2/3) Stage this hunk");
    session.input("k\\r");
    session.prompt("(1/3) Stage this hunk");
    session.input("/THIRD\\r");
    session.prompt("(3/3) Stage this hunk");
    session.input("y\\r");
    session.prompt("(1/3) Stage this hunk");
    session.input("q\\r");
    session.finish();
    repo.assert_index(&[(37, "THIRD change")]);
}

#[test]
fn edit_hunk_in_vim() {
    let repo = Repo::new();
    let worktree = repo.change(&[(3, "FIRST change"), (5, "SECOND change")]);
    let session = repo.patch(false);
    session.prompt("Stage this hunk");
    session.input("e\\r");
    session.expect("Manual hunk edit", false);
    // Keep the first replacement; turn the second removal into context and drop its addition.
    session.input(":g/^+SECOND change$/d\\r:%s/^-line 05$/ line 05/\\r:wq\\r");
    session.finish();
    repo.assert_index(&[(3, "FIRST change")]);
    assert_eq!(
        fs::read_to_string(repo.dir.path().join("tracked.txt")).unwrap(),
        worktree
    );
}

#[test]
fn abort_hunk_edit_and_quit() {
    let repo = Repo::new();
    repo.change(&[(3, "FIRST change")]);
    let session = repo.patch(false);
    session.prompt("Stage this hunk");
    session.input("e\\r");
    session.expect("Manual hunk edit", false);
    session.input(":%d\\r:wq\\r");
    session.prompt("Stage this hunk");
    session.input("q\\r");
    session.finish();
    repo.assert_index(&[]);
}

#[test]
fn untracked_file_requires_intent_to_add_then_can_be_edited() {
    let repo = Repo::new();
    fs::write(repo.dir.path().join("new.txt"), "keep new\nskip new\n").unwrap();
    let session = repo.patch(false);
    session.finish();
    assert!(session.screen().contains("No changes"));
    assert!(repo.git(&["ls-files", "new.txt"]).is_empty());
    drop(session);
    repo.git(&["add", "-N", "--", "new.txt"]);
    let session = repo.patch(false);
    session.prompt("Stage addition");
    session.input("e\\r");
    session.expect("Manual hunk edit", false);
    session.input(":g/^+skip new$/d\\r:wq\\r");
    session.finish();
    assert_eq!(repo.git(&["show", ":new.txt"]), "keep new\n");
    assert_eq!(
        fs::read_to_string(repo.dir.path().join("new.txt")).unwrap(),
        "keep new\nskip new\n"
    );
}

#[test]
fn failed_edit_can_be_retried() {
    let repo = Repo::new();
    repo.change(&[(3, "FIRST change")]);
    let session = repo.patch(false);
    session.prompt("Stage this hunk");
    session.input("e\\r");
    session.expect("Manual hunk edit", false);
    session.input(":%s/^ line 01$/ missing context/\\r:wq\\r");
    session.expect("Edit again", false);
    session.input("y\\r");
    session.expect("Manual hunk edit", false);
    // Git restores the original hunk for the next editing attempt.
    session.input(":wq\\r");
    session.finish();
    repo.assert_index(&[(3, "FIRST change")]);
}

#[test]
fn patch_from_interactive_add_menu() {
    let repo = Repo::new();
    repo.change(&[(3, "FIRST change")]);
    let session = repo.start(&["git", "-c", "interactive.singleKey=true", "add", "-i"]);
    session.prompt("What now>");
    // The menus require Enter even when patch decisions use single keys.
    session.input("p\\r");
    session.prompt("Patch update>>");
    session.input("1\\r");
    session.expect("* 1:", false);
    session.input("\\r");
    session.prompt("Stage this hunk");
    session.input("y");
    session.prompt("What now>");
    repo.assert_index(&[(3, "FIRST change")]);
    session.input("q\\r");
    session.finish();
}

#[test]
fn patch_from_persistent_shell() {
    let repo = Repo::new();
    repo.change(&[(3, "FIRST change")]);
    let session = repo.start(&["bash", "--noprofile", "--norc"]);
    session.prompt("READY>");
    session.input("git -c interactive.singleKey=false add -p\\r");
    session.prompt("Stage this hunk");
    session.input("y\\r");
    session.prompt("READY>");
    repo.assert_index(&[(3, "FIRST change")]);
    session.input("exit\\r");
    session.finish();
}
