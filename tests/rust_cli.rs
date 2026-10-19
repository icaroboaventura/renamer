use serde_json::{Value, json};
use std::{fs, process::Command};
fn cli(state: &std::path::Path, args: &[&str], ok: bool) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_renamer"))
        .arg("--cli")
        .arg("--data-dir")
        .arg(state)
        .args(args)
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    assert_eq!(output.status.success(), ok, "{v}");
    assert_eq!(v["ok"], ok);
    v
}
#[test]
fn cli_end_to_end() {
    let t = tempfile::tempdir().unwrap();
    let state = t.path().join("state");
    let file = t.path().join("MyFile.txt");
    fs::write(&file, "test").unwrap();
    let job = t.path().join("job.json");
    fs::write(
        &job,
        json!({"paths":[file],"rules":[{"kind":"Word format","options":{"mode":"snake_case"}}]})
            .to_string(),
    )
    .unwrap();
    let p = cli(
        &state,
        &["preview", "--request", job.to_str().unwrap()],
        true,
    );
    assert_eq!(p["changes"], 1);
    let id = p["plan_id"].as_str().unwrap();
    assert!(file.exists());
    let result = cli(&state, &["apply", "--plan", id], true);
    assert_eq!(result["batch_id"], id);
    assert!(t.path().join("my_file.txt").exists());
    cli(&state, &["apply", "--plan", id], false);
    assert_eq!(cli(&state, &["history"], true)["batch_id"], id);
    cli(&state, &["undo", "--batch", "wrong"], false);
    cli(&state, &["undo", "--batch", id], true);
    assert!(file.exists());
    cli(&state, &["apply", "--plan", id], false);
}
#[test]
fn regex_worker() {
    let t = tempfile::tempdir().unwrap();
    let v = cli(
        t.path(),
        &[
            "regex",
            "--pattern",
            r"^(\d+)-(.*)$",
            "--replacement",
            r"\g<2>_\g<1>",
            "--text",
            "12-Photo",
        ],
        true,
    );
    assert_eq!(v["output"], "Photo_12");
    cli(t.path(), &["regex", "--pattern", "[", "--text", "x"], false);
    let text = format!("{}!", "a".repeat(500));
    let started = std::time::Instant::now();
    cli(
        t.path(),
        &["regex", "--pattern", r"(a+)+$", "--text", &text],
        true,
    );
    assert!(started.elapsed().as_secs() < 5);
}
#[test]
fn ten_thousand_preview() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("files");
    fs::create_dir(&root).unwrap();
    for n in 0..10_000 {
        fs::write(root.join(format!("MyFile{n}.txt")), "").unwrap();
    }
    let job = t.path().join("job.json");
    fs::write(&job,json!({"paths":[root],"contents":true,"rules":[{"kind":"Word format","options":{"mode":"snake_case"}}]}).to_string()).unwrap();
    let start = std::time::Instant::now();
    let p = cli(
        &t.path().join("state"),
        &[
            "preview",
            "--request",
            job.to_str().unwrap(),
            "--limit",
            "2",
        ],
        true,
    );
    assert_eq!(p["total"], 10_000);
    assert_eq!(p["conflicts"], 0);
    assert_eq!(p["entries"].as_array().unwrap().len(), 2);
    eprintln!("10,000 item preview: {:?}", start.elapsed());
}
