//! Integration tests that drive the `vesting` binary with a temp state file.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const A: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const B: &str = "0202020202020202020202020202020202020202020202020202020202020202";
const TOKEN: &str = "0303030303030303030303030303030303030303030303030303030303030303";

struct Fixture {
    dir: tempfile::TempDir,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        Self { dir, state }
    }

    fn run(&self, now: u64, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_vesting"))
            .arg("--state")
            .arg(&self.state)
            .arg("--names")
            .arg(self.dir.path().join("names.json"))
            .arg("--now")
            .arg(now.to_string())
            .args(args)
            .output()
            .unwrap()
    }

    fn ok(&self, now: u64, args: &[&str]) -> String {
        let output = self.run(now, args);
        assert!(
            output.status.success(),
            "command failed: {:?}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn fail(&self, now: u64, args: &[&str]) -> String {
        let output = self.run(now, args);
        assert!(
            !output.status.success(),
            "command unexpectedly succeeded: {:?}",
            args
        );
        String::from_utf8(output.stderr).unwrap()
    }

    fn create_linear(&self, total: u128, start: u64, end: u64) {
        self.ok(
            start,
            &[
                "create",
                "--creator",
                A,
                "--beneficiary",
                B,
                "--token",
                TOKEN,
                "--total",
                &total.to_string(),
                "--kind",
                "linear",
                "--start",
                &start.to_string(),
                "--end",
                &end.to_string(),
            ],
        );
    }
}

#[test]
fn linear_lifecycle() {
    let f = Fixture::new();
    let created = f.ok(
        0,
        &[
            "create",
            "--creator",
            A,
            "--beneficiary",
            B,
            "--token",
            TOKEN,
            "--total",
            "1000",
            "--kind",
            "linear",
            "--start",
            "0",
            "--end",
            "1000",
        ],
    );
    assert!(created.contains("created schedule 0"));
    assert!(created.contains("total: 1000"));

    let claimable = f.ok(500, &["claimable", "0"]);
    assert!(claimable.contains("claimable now: 500 of 1000"));
    assert!(claimable.contains("next unlock in"));

    assert!(f.ok(500, &["claim", "0"]).contains("claimed 500"));
    assert!(f.ok(1_000, &["claim", "0"]).contains("claimed 500"));
    let failed = f.fail(1_001, &["claim", "0"]);
    assert!(failed.contains("code 9"));
    assert!(failed.contains("everything vested has been claimed"));
    assert!(f.ok(1_001, &["show", "0"]).contains("claimed: 1000"));

    let listed = f.ok(1_001, &["list", "--beneficiary", B]);
    assert!(listed.contains("schedule 0"));
    assert!(f
        .ok(1_001, &["list", "--beneficiary", A])
        .contains("no schedules"));
}

#[test]
fn cliff_linear_lifecycle() {
    let f = Fixture::new();
    f.ok(
        0,
        &[
            "create",
            "--creator",
            A,
            "--beneficiary",
            B,
            "--token",
            TOKEN,
            "--total",
            "1000",
            "--kind",
            "cliff-linear",
            "--start",
            "0",
            "--cliff",
            "5000",
            "--end",
            "10000",
        ],
    );

    let before = f.fail(1_000, &["claim", "0"]);
    assert!(before.contains("the cliff is at 5000 ms"));
    assert!(before.contains("waiting for the cliff"));
    assert!(f
        .ok(1_000, &["claimable", "0"])
        .contains("claimable now: 0 of 1000"));

    assert!(f.ok(5_000, &["claim", "0"]).contains("claimed 500"));

    let cancelled = f.ok(6_000, &["cancel", "0"]);
    assert!(cancelled.contains("refunded 400 to the creator"));
    assert!(cancelled.contains("100 remains claimable"));

    assert!(f.ok(9_999, &["claim", "0"]).contains("claimed 100"));
    let failed = f.fail(10_000, &["claim", "0"]);
    assert!(failed.contains("cancelled and everything vested at cancellation"));
    assert!(f.ok(10_000, &["show", "0"]).contains("cancelled at"));
}

#[test]
fn milestone_lifecycle() {
    let f = Fixture::new();
    f.ok(
        0,
        &[
            "create",
            "--creator",
            A,
            "--beneficiary",
            B,
            "--token",
            TOKEN,
            "--total",
            "100",
            "--kind",
            "milestone",
            "--tranche",
            "40",
            "--tranche",
            "60",
        ],
    );

    let failed = f.fail(0, &["claim", "0"]);
    assert!(failed.contains("milestone authority"));
    assert!(failed.contains("status: waiting for a milestone signal"));
    assert!(f
        .ok(0, &["claimable", "0"])
        .contains("next unlock: awaiting a milestone signal"));

    assert!(f
        .ok(0, &["signal-milestone", "0", "0"])
        .contains("unlocked 40"));
    assert!(f.ok(0, &["claim", "0"]).contains("claimed 40"));
    let twice = f.fail(0, &["signal-milestone", "0", "0"]);
    assert!(twice.contains("code 18"));
    assert!(twice.contains("cannot unlock twice"));

    assert!(f
        .ok(0, &["signal-milestone", "0", "1"])
        .contains("unlocked 60"));
    assert!(f.ok(0, &["claim", "0"]).contains("claimed 60"));
    let done = f.fail(0, &["claim", "0"]);
    assert!(
        done.contains("already been claimed"),
        "unexpected stderr: {}",
        done
    );
}

#[test]
fn create_batch_and_authorities() {
    let f = Fixture::new();
    let file = f.dir.path().join("batch.json");
    let json = format!(
        r#"[
            {{"creator":"{}","beneficiary":"{}","token":"{}","total":"10",
              "kind":{{"type":"linear","start":0,"end":10}}}},
            {{"creator":"{}","beneficiary":"{}","token":"{}","total":"20",
              "kind":{{"type":"milestone","tranches":["5","15"]}},
              "cancel_authority":"{}","transferable":true}}
        ]"#,
        A, B, TOKEN, A, B, TOKEN, B
    );
    fs::write(&file, json).unwrap();
    let out = f.ok(0, &["create-batch", file.to_str().unwrap()]);
    assert!(out.contains("created schedule 0"));
    assert!(out.contains("created schedule 1"));
    assert!(f.ok(0, &["list"]).contains("schedule 1"));

    // The second schedule has the beneficiary as its cancel authority.
    assert!(f.ok(0, &["cancel", "1"]).contains("cancelled schedule 1"));
    // The first one does not.
    assert!(f
        .fail(0, &["cancel", "0", "--as-account", B])
        .contains("code 11"));
}

#[test]
fn private_claim_needs_confirmation_and_prints_disclosure() {
    let f = Fixture::new();
    f.create_linear(1_000, 0, 1_000);

    // Answering "n" at the prompt must not submit a claim.
    let output = Command::new(env!("CARGO_BIN_EXE_vesting"))
        .arg("--state")
        .arg(&f.state)
        .arg("--names")
        .arg(f.dir.path().join("names.json"))
        .arg("--now")
        .arg("500")
        .args(["claim", "0", "--to-private", B])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = output;
    child.stdin.take().unwrap().write_all(b"n\n").unwrap();
    let declined = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(declined.stdout).unwrap();
    assert!(declined.status.success());
    assert!(stdout.contains("Privacy disclosure"));
    assert!(stdout.contains("Visible on-chain:"));
    assert!(stdout.contains("Not traceable:"));
    assert!(stdout.contains("cancelled: no claim was submitted"));
    assert!(f.ok(500, &["show", "0"]).contains("claimed: 0"));

    // With --yes the claim goes through and the event records the flag.
    let accepted = f.ok(500, &["claim", "0", "--to-private", B, "--yes"]);
    assert!(accepted.contains("Privacy disclosure"));
    assert!(accepted.contains("destination:"));
    assert!(accepted.contains("(private)"));
    assert!(f.ok(500, &["show", "0"]).contains("claimed: 500"));
}

#[test]
fn transfer_and_irrevocable_flags() {
    let f = Fixture::new();
    f.ok(
        0,
        &[
            "create",
            "--creator",
            A,
            "--beneficiary",
            B,
            "--token",
            TOKEN,
            "--total",
            "100",
            "--kind",
            "linear",
            "--start",
            "0",
            "--end",
            "100",
            "--transferable",
            "true",
        ],
    );
    assert!(f
        .fail(0, &["transfer", "0", A, "--as-account", A])
        .contains("code 10"));
    assert!(f
        .ok(0, &["transfer", "0", A])
        .contains("beneficiary of schedule 0 is now"));
    // The old beneficiary can no longer claim; the new one can.
    assert!(f
        .fail(50, &["claim", "0", "--as-account", B])
        .contains("code 10"));
    assert!(f.ok(50, &["claim", "0"]).contains("claimed 50"));
}
