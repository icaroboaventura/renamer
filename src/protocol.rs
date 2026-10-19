use crate::{engine::*, rules::*};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::File,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
#[derive(Serialize, Deserialize)]
pub struct Plan {
    pub version: u32,
    pub plan_id: String,
    pub request: Request,
    pub entries: Vec<Entry>,
    #[serde(default)]
    pub submitted: bool,
}
pub fn worker(job: Value, cancel: &AtomicBool) -> Result<Value> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("input.json");
    let output = dir.path().join("output.json");
    save_json(&input, &job)?;
    let mut child = Command::new(std::env::current_exe()?)
        .arg("--worker")
        .arg(&input)
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let start = Instant::now();
    let timeout = if job["kind"] == "regex" { 3 } else { 30 };
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!("Evaluation worker failed");
            }
            break;
        }
        if cancel.load(std::sync::atomic::Ordering::Relaxed)
            || start.elapsed() > Duration::from_secs(timeout)
        {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Evaluation cancelled or exceeded its time limit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let value: Value = serde_json::from_reader(File::open(output)?)?;
    if value["ok"] != true {
        bail!("{}", value["error"].as_str().unwrap_or("Evaluation failed"));
    }
    Ok(value)
}
pub fn run_worker(input: &Path, output: &Path) -> Result<()> {
    let job: Value = serde_json::from_reader(File::open(input)?)?;
    let result = (|| -> Result<Value> {
        if job["kind"] == "regex" {
            let rule = Rule {
                kind: "Regular expression".into(),
                enabled: true,
                target: "Name".into(),
                options: job["options"].clone(),
            };
            let compiled = CompiledRule::new(rule)?;
            let (result, matches) = compiled.replace(job["text"].as_str().unwrap_or(""))?;
            Ok(json!({"ok":true,"result":result,"output":result,"matches":matches}))
        } else {
            let entries = build_plan(
                serde_json::from_value(job["items"].clone())?,
                serde_json::from_value(job["rules"].clone())?,
                job["compound"].as_bool().unwrap_or(true),
            )?;
            Ok(json!({"ok":true,"entries":entries}))
        }
    })();
    save_json(
        output,
        &result.unwrap_or_else(|e| json!({"ok":false,"error":format!("{e:#}")})),
    )
}
pub fn preview(
    items: Vec<Item>,
    rules: Vec<Rule>,
    compound: bool,
    cancel: &AtomicBool,
) -> Result<Vec<Entry>> {
    Ok(serde_json::from_value(
        worker(
            json!({"kind":"preview","items":items,"rules":rules,"compound":compound}),
            cancel,
        )?["entries"]
            .take(),
    )?)
}
pub fn plan_path(engine: &Engine, id: &str) -> Result<PathBuf> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        bail!("Invalid plan ID");
    }
    Ok(engine.root.join("plans").join(format!("{id}.json")))
}
pub fn load(engine: &Engine, id: &str) -> Result<Plan> {
    let p: Plan = serde_json::from_reader(File::open(plan_path(engine, id)?)?)?;
    if p.version != 1 || p.plan_id != id {
        bail!("Invalid plan");
    }
    Ok(p)
}
pub fn describe(plan: &Plan, offset: usize, limit: usize) -> Value {
    let errors = plan.entries.iter().filter(|e| !e.error.is_empty()).count();
    let changes = plan
        .entries
        .iter()
        .filter(|e| basename(&e.item.path) != e.name)
        .count();
    json!({"ok":true,"plan_id":plan.plan_id,"total":plan.entries.len(),"changes":changes,"conflicts":errors,"can_apply":!plan.entries.is_empty()&&errors==0,"offset":offset,"has_more":offset.saturating_add(limit)<plan.entries.len(),"entries":plan.entries.iter().skip(offset).take(limit).map(|e|json!({"source":e.item.path,"destination":e.final_path,"error":e.error})).collect::<Vec<_>>(),"submitted":plan.submitted,"status":if plan.submitted{if changes>0{"complete"}else{"unchanged"}}else{"preview"},"batch_id":if plan.submitted&&changes>0{Some(&plan.plan_id)}else{None}})
}
pub fn request_file(path: &str) -> Result<Request> {
    if path == "-" {
        Ok(serde_json::from_reader(std::io::stdin())?)
    } else {
        Ok(serde_json::from_reader(File::open(path)?)?)
    }
}
pub fn create(engine: &Engine, request: Request, cancel: &AtomicBool) -> Result<Plan> {
    request.validate()?;
    let mut items = scan(&request, cancel)?;
    sort_items(&mut items, &request.order, request.descending);
    if items.is_empty() {
        bail!("No items matched the request");
    }
    if items
        .iter()
        .any(|i| i.is_dir && engine.root.starts_with(&i.path))
    {
        bail!("Choose a data directory outside selected folders");
    }
    let entries = preview(
        items,
        request.rules.clone(),
        request.compound_extensions,
        cancel,
    )?;
    let plan = Plan {
        version: 1,
        plan_id: id(),
        request,
        entries,
        submitted: false,
    };
    save_json(&plan_path(engine, &plan.plan_id)?, &plan)?;
    Ok(plan)
}
pub fn apply(engine: &Engine, id: &str, cancel: &AtomicBool) -> Result<Plan> {
    let mut plan = load(engine, id)?;
    if plan.submitted {
        bail!("This plan has already been submitted");
    }
    engine.execute(&mut plan.entries, id, cancel, |_, _| {})?;
    plan.submitted = true;
    save_json(&plan_path(engine, id)?, &plan)?;
    Ok(plan)
}
