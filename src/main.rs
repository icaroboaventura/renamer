mod gui;
use anyhow::Result;
use clap::{Parser, Subcommand};
use renamer::{engine::Engine, protocol, rules};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
#[derive(Parser)]
#[command(version, about = "Renamer — local batch file renaming in Rust")]
struct Args {
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[arg(long)]
    request: Option<String>,
    #[command(subcommand)]
    command: Option<Commands>,
}
#[derive(Subcommand)]
enum Commands {
    Catalog,
    Preview {
        #[arg(long)]
        request: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    Inspect {
        #[arg(long)]
        plan: String,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    Apply {
        #[arg(long)]
        plan: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    History,
    Undo {
        #[arg(long)]
        batch: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    Regex {
        #[arg(long)]
        pattern: String,
        #[arg(long, default_value = "")]
        replacement: String,
        #[arg(long)]
        text: String,
        #[arg(long)]
        ignore_case: bool,
        #[arg(long)]
        first: bool,
    },
    Open {
        #[arg(long)]
        request: String,
    },
}
fn run() -> Result<()> {
    let raw: Vec<String> = std::env::args().collect();
    if raw.get(1).is_some_and(|s| s == "--worker") {
        return protocol::run_worker(std::path::Path::new(&raw[2]), std::path::Path::new(&raw[3]));
    }
    let args = Args::parse_from(raw.into_iter().filter(|s| s != "--cli"));
    let engine = Engine::new(args.data_dir)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    ctrlc::set_handler(move || c.store(true, std::sync::atomic::Ordering::Relaxed))?;
    let result = match args.command {
        None => {
            return gui::launch(
                engine,
                args.request
                    .map(|p| protocol::request_file(&p))
                    .transpose()?,
            );
        }
        Some(Commands::Open { request }) => {
            protocol::request_file(&request)?.validate()?;
            std::process::Command::new(std::env::current_exe()?)
                .arg("--data-dir")
                .arg(&engine.root)
                .arg("--request")
                .arg(&request)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            json!({"ok":true,"status":"launched","request":request,"note":"Opens a preview only; does not rename files"})
        }
        Some(Commands::Catalog) => {
            let mut v = rules::catalog();
            v["ok"] = json!(true);
            v
        }
        Some(Commands::Preview { request, limit }) => protocol::describe(
            &protocol::create(&engine, protocol::request_file(&request)?, &cancel)?,
            0,
            limit,
        ),
        Some(Commands::Inspect {
            plan,
            offset,
            limit,
        }) => protocol::describe(&protocol::load(&engine, &plan)?, offset, limit),
        Some(Commands::Apply { plan, limit }) => {
            protocol::describe(&protocol::apply(&engine, &plan, &cancel)?, 0, limit)
        }
        Some(Commands::History) => match engine.read()? {
            Some(r) => {
                json!({"ok":true,"batch_id":r.batch_id,"status":r.status,"items":r.entries.len(),"error":r.error})
            }
            None => json!({"ok":true,"status":"empty","batch_id":null}),
        },
        Some(Commands::Undo { batch, limit }) => {
            let paths = engine.undo(batch.as_deref(), |_, _| {})?;
            json!({"ok":true,"total":paths.len(),"restored":paths.len(),"status":"undone","batch_id":batch,"paths":paths.into_iter().take(limit).collect::<Vec<_>>()})
        }
        Some(Commands::Regex {
            pattern,
            replacement,
            text,
            ignore_case,
            first,
        }) => protocol::worker(
            json!({"kind":"regex","options":{"pattern":pattern,"replacement":replacement,"ignore_case":ignore_case,"first":first},"text":text}),
            &cancel,
        )?,
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        println!(
            "{}",
            json!({"ok":false,"error":format!("{error:#}"),"error_type":"RenamerError"})
        );
        std::process::exit(1);
    }
}
