use renamer::{engine::*, rules::*};
use serde_json::json;
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
fn rule(kind: &str, options: serde_json::Value) -> Rule {
    Rule {
        kind: kind.into(),
        options,
        enabled: true,
        target: "Name".into(),
    }
}
fn files(root: &Path, names: &[&str]) -> Vec<Item> {
    names
        .iter()
        .map(|n| {
            let p = root.join(n);
            fs::write(&p, n).unwrap();
            Item::read(&p).unwrap()
        })
        .collect()
}
fn transform(text: &str, r: Rule) -> String {
    CompiledRule::new(r).unwrap().transform(text, 0).unwrap()
}
#[test]
fn formats() {
    for (mode, want) in [
        ("camelCase", "httpServer"),
        ("PascalCase", "HttpServer"),
        ("snake_case", "http_server"),
        ("SCREAMING_SNAKE_CASE", "HTTP_SERVER"),
        ("kebab-case", "http-server"),
        ("Train-Case", "Http-Server"),
        ("Separate words", "HTTP Server"),
        ("Join words", "HTTPServer"),
    ] {
        assert_eq!(
            transform("HTTPServer", rule("Word format", json!({"mode":mode}))),
            want
        );
    }
}
#[test]
fn operations() {
    for (kind, options, input, want) in [
        (
            "Case",
            json!({"mode":"Title Case"}),
            "hello world",
            "Hello World",
        ),
        (
            "Case",
            json!({"mode":"Sentence case"}),
            "hELLO WORLD",
            "Hello world",
        ),
        ("Case", json!({"mode":"Invert case"}), "aB", "Ab"),
        (
            "Remove",
            json!({"mode":"Characters at position","position":-3,"count":2}),
            "abcdef",
            "abcf",
        ),
        ("Remove", json!({"mode":"Digits"}), "a12b", "ab"),
        ("Remove", json!({"mode":"Punctuation"}), "a-b_c!", "abc"),
        (
            "Whitespace",
            json!({"mode":"Remove all spaces"}),
            "a \t b",
            "ab",
        ),
        ("Whitespace", json!({"mode":"Trim ends"}), " a b ", "a b"),
        (
            "Whitespace",
            json!({"mode":"Collapse spaces"}),
            "a \t b",
            "a b",
        ),
        (
            "Whitespace",
            json!({"mode":"Spaces to separator","separator":"-"}),
            "a b",
            "a-b",
        ),
        (
            "Insert",
            json!({"mode":"At position","text":"X","position":1}),
            "ab",
            "aXb",
        ),
        (
            "Numbering",
            json!({"mode":"Suffix","start":7,"digits":3,"separator":"-"}),
            "a",
            "a-007",
        ),
        (
            "Replace text",
            json!({"pattern":" ","replacement":"\\"}),
            "a b",
            "a\\b",
        ),
    ] {
        assert_eq!(transform(input, rule(kind, options)), want);
    }
}
#[test]
fn chain() {
    let rules = vec![
        rule("Remove", json!({"mode":"Accents"})),
        rule(
            "Word format",
            json!({"mode":"snake_case","split_numbers":true}),
        ),
        rule("Numbering", json!({"mode":"Prefix","start":3,"digits":2})),
    ]
    .into_iter()
    .map(|r| CompiledRule::new(r).unwrap())
    .collect::<Vec<_>>();
    assert_eq!(
        apply("MeuÁlbum12.tar.gz", false, &rules, 0, 0, true).unwrap(),
        "03_meu_album_12.tar.gz"
    );
}
#[test]
fn regex_examples() {
    for e in catalog()["regex_examples"].as_array().unwrap() {
        assert_eq!(
            transform(
                e["input"].as_str().unwrap(),
                rule(
                    "Regular expression",
                    json!({"pattern":e["pattern"],"replacement":e["replacement"]})
                )
            ),
            e["output"].as_str().unwrap()
        );
    }
    assert!(CompiledRule::new(rule("Regular expression", json!({"pattern":"["}))).is_err());
    assert!(
        CompiledRule::new(rule(
            "Regular expression",
            json!({"pattern":"x","replacement":"\\g<3>"})
        ))
        .is_err()
    );
    assert_eq!(
        transform(
            "aaa",
            rule(
                "Regular expression",
                json!({"pattern":"a","replacement":"$1","first":true})
            )
        ),
        "$1aa"
    );
}
#[test]
fn case_and_restart_undo() {
    let t = tempfile::tempdir().unwrap();
    let mut p = build_plan(
        files(t.path(), &["one.txt", "two.txt"]),
        vec![rule("Case", json!({"mode":"UPPERCASE"}))],
        true,
    )
    .unwrap();
    let e = Engine::new(Some(t.path().join("state"))).unwrap();
    e.execute(&mut p, "test", &AtomicBool::new(false), |_, _| {})
        .unwrap();
    assert_eq!(
        fs::read_to_string(t.path().join("ONE.txt")).unwrap(),
        "one.txt"
    );
    Engine::new(Some(e.root))
        .unwrap()
        .undo(Some("test"), |_, _| {})
        .unwrap();
    assert!(t.path().join("one.txt").exists());
}
#[test]
fn swaps() {
    let t = tempfile::tempdir().unwrap();
    let items = files(t.path(), &["a", "b"]);
    let mut p: Vec<_> = items
        .into_iter()
        .zip(["b", "a"])
        .map(|(item, n)| Entry {
            item,
            name: n.into(),
            final_path: String::new(),
            error: String::new(),
        })
        .collect();
    let e = Engine::new(Some(t.path().join("state"))).unwrap();
    e.execute(&mut p, "swap", &AtomicBool::new(false), |_, _| {})
        .unwrap();
    assert_eq!(fs::read_to_string(t.path().join("a")).unwrap(), "b");
    e.undo(None, |_, _| {}).unwrap();
    assert_eq!(fs::read_to_string(t.path().join("a")).unwrap(), "a");
}
#[test]
fn nested() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("Folder");
    let c = p.join("Child");
    fs::create_dir_all(&c).unwrap();
    let f = c.join("File.txt");
    fs::write(&f, "data").unwrap();
    let mut plan = build_plan(
        [&p, &c, &f]
            .into_iter()
            .map(|p| Item::read(p).unwrap())
            .collect(),
        vec![rule("Case", json!({"mode":"UPPERCASE"}))],
        true,
    )
    .unwrap();
    let e = Engine::new(Some(t.path().join("state"))).unwrap();
    e.execute(&mut plan, "nested", &AtomicBool::new(false), |_, _| {})
        .unwrap();
    assert!(t.path().join("FOLDER/CHILD/FILE.txt").exists());
    e.undo(None, |_, _| {}).unwrap();
    assert!(f.exists());
}
#[test]
fn conflicts_and_no_overwrite() {
    let t = tempfile::tempdir().unwrap();
    let items = files(t.path(), &["a", "b"]);
    assert!(rename_exclusive(Path::new(&items[0].path), Path::new(&items[1].path)).is_err());
    let p = build_plan(
        items,
        vec![rule(
            "Insert",
            json!({"mode":"Replace entire name","text":"same"}),
        )],
        true,
    )
    .unwrap();
    assert!(p.iter().all(|e| !e.error.is_empty()));
}
#[test]
fn unicode_conflicts() {
    let t = tempfile::tempdir().unwrap();
    let mut p: Vec<_> = files(t.path(), &["a", "b"])
        .into_iter()
        .zip(["Café", "Cafe\u{301}"])
        .map(|(item, name)| Entry {
            item,
            name: name.into(),
            final_path: String::new(),
            error: String::new(),
        })
        .collect();
    validate(&mut p).unwrap();
    assert!(p.iter().all(|e| !e.error.is_empty()));
}
#[test]
fn cancellation_rolls_back() {
    let t = tempfile::tempdir().unwrap();
    let mut p = build_plan(
        files(t.path(), &["a", "b"]),
        vec![rule("Case", json!({"mode":"UPPERCASE"}))],
        true,
    )
    .unwrap();
    let e = Engine::new(Some(t.path().join("state"))).unwrap();
    let cancel = AtomicBool::new(false);
    assert!(
        e.execute(&mut p, "cancel", &cancel, |n, _| {
            if n == 1 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .is_err()
    );
    assert_eq!(fs::read_to_string(t.path().join("a")).unwrap(), "a");
    assert_eq!(e.read().unwrap().unwrap().status, "undone");
}
#[test]
fn failure_rolls_back() {
    let t = tempfile::tempdir().unwrap();
    let mut p = build_plan(
        files(t.path(), &["a", "b"]),
        vec![rule("Insert", json!({"mode":"Prefix","text":"new_"}))],
        true,
    )
    .unwrap();
    let e = Engine::new(Some(t.path().join("state"))).unwrap();
    assert!(
        e.execute(&mut p, "failure", &AtomicBool::new(false), |n, _| {
            if n == 2 {
                fs::write(t.path().join("new_a"), "external").unwrap();
            }
        })
        .is_err()
    );
    assert_eq!(fs::read_to_string(t.path().join("a")).unwrap(), "a");
    assert_eq!(
        fs::read_to_string(t.path().join("new_a")).unwrap(),
        "external"
    );
}
#[test]
fn pending_recovery() {
    let t = tempfile::tempdir().unwrap();
    let item = files(t.path(), &["a"]).remove(0);
    let e = Engine::new(Some(t.path().join("state"))).unwrap();
    let dst = t.path().join("temp");
    save_json(&e.root.join("history.json"),&json!({"status":"running","entries":[],"steps":[{"source":item.path,"destination":dst,"identity":item.identity,"state":"pending"}]})).unwrap();
    rename_exclusive(Path::new(&item.path), &dst).unwrap();
    e.undo(None, |_, _| {}).unwrap();
    assert!(Path::new(&item.path).exists());
}
#[test]
fn stale_and_undo_collision() {
    let t = tempfile::tempdir().unwrap();
    let mut p = build_plan(
        files(t.path(), &["a"]),
        vec![rule("Insert", json!({"mode":"Prefix","text":"new_"}))],
        true,
    )
    .unwrap();
    let e = Engine::new(Some(t.path().join("state"))).unwrap();
    fs::write(t.path().join("a"), "changed").unwrap();
    assert!(
        e.execute(&mut p, "stale", &AtomicBool::new(false), |_, _| {})
            .is_err()
    );
    p[0].item = Item::read(&t.path().join("a")).unwrap();
    e.execute(&mut p, "fresh", &AtomicBool::new(false), |_, _| {})
        .unwrap();
    fs::write(t.path().join("a"), "other").unwrap();
    assert!(e.undo(None, |_, _| {}).is_err());
    assert_eq!(
        fs::read_to_string(t.path().join("new_a")).unwrap(),
        "changed"
    );
}
#[test]
fn scan_filters_and_packages() {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir(t.path().join("Test.app")).unwrap();
    fs::write(t.path().join("Test.app/secret"), "").unwrap();
    files(t.path(), &[".hidden", "visible.txt"]);
    std::os::unix::fs::symlink(t.path().join("Test.app"), t.path().join("link")).unwrap();
    let r = Request {
        paths: vec![t.path().to_str().unwrap().into()],
        contents: true,
        recursive: true,
        ..Default::default()
    };
    let items = scan(&r, &AtomicBool::new(false)).unwrap();
    assert_eq!(items.len(), 3);
    assert!(items.iter().all(|i| basename(&i.path) != "secret"));
}

#[test]
fn extension_targets_and_folder_numbering() {
    let t = tempfile::tempdir().unwrap();
    let first = t.path().join("first");
    let second = t.path().join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let mut selected = files(&first, &["a.TXT", "b.TXT"]);
    selected.extend(files(&second, &["c.TXT"]));
    let mut extension = rule("Case", json!({"mode":"lowercase"}));
    extension.target = "Extension".into();
    let p = build_plan(
        selected,
        vec![
            extension,
            rule(
                "Numbering",
                json!({"mode":"Prefix","per_folder":true,"digits":2}),
            ),
        ],
        true,
    )
    .unwrap();
    assert_eq!(
        p.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["01_a.txt", "02_b.txt", "01_c.txt"]
    );
    assert_eq!(split_name(".hidden", false, true), (".hidden", ""));
    assert_eq!(
        split_name("Archive.tar.gz", false, true),
        ("Archive", ".tar.gz")
    );
    assert_eq!(
        split_name("Archive.tar.gz", true, true),
        ("Archive.tar.gz", "")
    );
}

#[test]
fn desktop_folder_contents_and_optional_root() {
    let t = tempfile::tempdir().unwrap();
    let folder = t.path().join("MyFolder");
    fs::create_dir(&folder).unwrap();
    files(&folder, &["FirstFile.txt", "SecondFile.txt"]);
    let sub = folder.join("SubFolder");
    fs::create_dir(&sub).unwrap();
    files(&sub, &["DeepFile.txt"]);
    let request = Request {
        paths: vec![folder.to_str().unwrap().into()],
        contents: true,
        ..Default::default()
    };
    let (items, roots) = scan_folder_selection(&request, &AtomicBool::new(false)).unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(roots.len(), 1);
    assert_eq!(
        roots[0].path,
        folder.canonicalize().unwrap().to_str().unwrap()
    );
    assert!(items.iter().all(|i| i.path != roots[0].path));
    let recursive = Request {
        recursive: true,
        ..request.clone()
    };
    assert_eq!(
        scan_folder_selection(&recursive, &AtomicBool::new(false))
            .unwrap()
            .0
            .len(),
        4
    );
    let explicit = Request {
        contents: false,
        ..request
    };
    let (items, roots) = scan_folder_selection(&explicit, &AtomicBool::new(false)).unwrap();
    assert_eq!(items.len(), 1);
    assert!(roots.is_empty());
}
