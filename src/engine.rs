use crate::rules::{CompiledRule, Rule};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

pub fn stopped(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        bail!("Cancelled");
    }
    Ok(())
}
pub fn id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
pub fn path_string(path: &Path) -> Result<String> {
    Ok(path
        .to_str()
        .context("File paths must be valid Unicode")?
        .to_string())
}
pub fn basename(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
}
pub fn parent(path: &str) -> &Path {
    Path::new(path).parent().unwrap_or(Path::new("/"))
}
pub fn canonical(name: &str) -> String {
    name.nfd().case_fold().collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub path: String,
    pub is_dir: bool,
    pub identity: [u64; 2],
    pub mtime: i64,
    pub size: u64,
}
impl Item {
    pub fn read(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            bail!("An absolute file path is required");
        }
        let path = if let Some(name) = path.file_name() {
            path.parent()
                .context("Missing parent")?
                .canonicalize()?
                .join(name)
        } else {
            path.to_path_buf()
        };
        let m = fs::symlink_metadata(&path)
            .with_context(|| format!("Cannot read {}", path.display()))?;
        Ok(Self {
            path: path_string(&path)?,
            is_dir: m.is_dir(),
            identity: [m.dev(), m.ino()],
            mtime: m
                .mtime()
                .saturating_mul(1_000_000_000)
                .saturating_add(m.mtime_nsec()),
            size: m.size(),
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub item: Item,
    pub name: String,
    #[serde(default)]
    pub final_path: String,
    #[serde(default)]
    pub error: String,
}
fn default_kind() -> String {
    "Both".into()
}
fn default_order() -> String {
    "Name".into()
}
fn yes() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub paths: Vec<String>,
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub contents: bool,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub name_filter: String,
    #[serde(default)]
    pub extensions: String,
    #[serde(default = "default_order")]
    pub order: String,
    #[serde(default)]
    pub descending: bool,
    #[serde(default = "yes")]
    pub compound_extensions: bool,
}
impl Default for Request {
    fn default() -> Self {
        Self {
            paths: vec![],
            rules: vec![],
            contents: false,
            recursive: false,
            hidden: false,
            kind: default_kind(),
            name_filter: String::new(),
            extensions: String::new(),
            order: default_order(),
            descending: false,
            compound_extensions: true,
        }
    }
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        if self.paths.is_empty() || self.paths.iter().any(|p| !Path::new(p).is_absolute()) {
            bail!("paths must contain absolute local file/folder paths");
        }
        if self.rules.is_empty() {
            bail!("rules must be a nonempty array");
        }
        if !["Both", "Files", "Folders"].contains(&self.kind.as_str()) {
            bail!("Invalid item kind");
        }
        if !["Name", "Path", "Modified", "Manual"].contains(&self.order.as_str()) {
            bail!("Invalid order");
        }
        for rule in &self.rules {
            rule.validate()?;
        }
        Ok(())
    }
}
pub fn scan(request: &Request, cancel: &AtomicBool) -> Result<Vec<Item>> {
    struct Scanner<'a> {
        request: &'a Request,
        cancel: &'a AtomicBool,
        items: Vec<Item>,
        seen: HashSet<String>,
        pattern: Option<glob::Pattern>,
        extensions: HashSet<String>,
    }
    impl Scanner<'_> {
        fn add(&mut self, path: &Path) -> Result<()> {
            stopped(self.cancel)?;
            let item = Item::read(path)?;
            let name = basename(&item.path);
            if !self.request.hidden && name.starts_with('.') {
                return Ok(());
            }
            if (self.request.kind == "Files" && item.is_dir)
                || (self.request.kind == "Folders" && !item.is_dir)
            {
                return Ok(());
            }
            if self
                .pattern
                .as_ref()
                .is_some_and(|p| !p.matches(&canonical(name)))
            {
                return Ok(());
            }
            if !self.extensions.is_empty()
                && (item.is_dir
                    || !self.extensions.contains(
                        &Path::new(name)
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_lowercase(),
                    ))
            {
                return Ok(());
            }
            if self.seen.insert(item.path.clone()) {
                self.items.push(item);
            }
            Ok(())
        }
        fn walk(&mut self, path: &Path) -> Result<()> {
            let mut children = fs::read_dir(path)?
                .map(|e| e.map(|e| e.path()))
                .collect::<std::io::Result<Vec<_>>>()?;
            children.sort();
            for child in children {
                stopped(self.cancel)?;
                let name = child
                    .file_name()
                    .context("Invalid child path")?
                    .to_str()
                    .context("File names must be valid Unicode")?;
                if !self.request.hidden && name.starts_with('.') {
                    continue;
                }
                self.add(&child)?;
                if self.request.recursive
                    && fs::symlink_metadata(&child)?.is_dir()
                    && !name.to_lowercase().ends_with(".app")
                {
                    self.walk(&child)?;
                }
            }
            Ok(())
        }
    }
    let mut scan = Scanner {
        request,
        cancel,
        items: vec![],
        seen: HashSet::new(),
        pattern: if request.name_filter.is_empty() {
            None
        } else {
            Some(glob::Pattern::new(&canonical(&request.name_filter))?)
        },
        extensions: request
            .extensions
            .split(',')
            .map(|x| x.trim().trim_start_matches('.').to_lowercase())
            .filter(|x| !x.is_empty())
            .collect(),
    };
    for value in &request.paths {
        let path = Path::new(value);
        if request.contents
            && fs::symlink_metadata(path)?.is_dir()
            && !basename(value).to_lowercase().ends_with(".app")
        {
            scan.walk(path)?;
        } else {
            scan.add(path)?;
        }
    }
    Ok(scan.items)
}
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let a = canonical(a);
    let b = canonical(b);
    let mut a = a.chars().peekable();
    let mut b = b.chars().peekable();
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut n = String::new();
                let mut m = String::new();
                while a.peek().is_some_and(char::is_ascii_digit) {
                    n.push(a.next().unwrap());
                }
                while b.peek().is_some_and(char::is_ascii_digit) {
                    m.push(b.next().unwrap());
                }
                let n = n.trim_start_matches('0');
                let m = m.trim_start_matches('0');
                let order = n.len().cmp(&m.len()).then_with(|| n.cmp(m));
                if !order.is_eq() {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.cmp(&y);
                if !order.is_eq() {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}
pub fn sort_items(items: &mut [Item], order: &str, descending: bool) {
    if order == "Manual" {
        return;
    }
    items.sort_by(|a, b| {
        let result = match order {
            "Modified" => a.mtime.cmp(&b.mtime),
            "Path" => natural(&a.path, &b.path),
            _ => natural(basename(&a.path), basename(&b.path)),
        }
        .then_with(|| natural(&a.path, &b.path));
        if descending { result.reverse() } else { result }
    });
}
pub fn build_plan(items: Vec<Item>, rules: Vec<Rule>, compound: bool) -> Result<Vec<Entry>> {
    let compiled = rules
        .into_iter()
        .filter(|r| r.enabled)
        .map(CompiledRule::new)
        .collect::<Result<Vec<_>>>()?;
    let mut counts = HashMap::new();
    let mut entries = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        let count = counts
            .entry(parent(&item.path).to_path_buf())
            .or_insert(0usize);
        let result = crate::rules::apply(
            basename(&item.path),
            item.is_dir,
            &compiled,
            index,
            *count,
            compound,
        );
        *count += 1;
        let (name, error) = match result {
            Ok(name) => (name, String::new()),
            Err(e) => (basename(&item.path).to_string(), format!("{e:#}")),
        };
        entries.push(Entry {
            item,
            name,
            final_path: String::new(),
            error,
        });
    }
    validate(&mut entries)?;
    Ok(entries)
}
pub fn validate(entries: &mut [Entry]) -> Result<()> {
    let map: HashMap<String, String> = entries
        .iter()
        .map(|e| (e.item.path.clone(), e.name.clone()))
        .collect();
    if map.len() != entries.len() {
        bail!("Duplicate source paths in plan");
    }
    let mut groups: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
    for (index, e) in entries.iter_mut().enumerate() {
        if e.name.is_empty()
            || [".", ".."].contains(&e.name.as_str())
            || e.name.contains(['/', ':', '\0'])
        {
            e.error = "Invalid or empty name".into();
        }
        if e.name.len() > 255 {
            e.error = "Name exceeds 255 bytes".into();
        }
        if !Path::new(&e.item.path).is_absolute() || Path::new(&e.item.path).file_name().is_none() {
            e.error = "Cannot rename the filesystem root or a relative path".into();
        }
        let mut parts = vec![];
        let mut current = Path::new(&e.item.path);
        while let Some(name) = current.file_name() {
            parts.push(
                map.get(&path_string(current)?)
                    .cloned()
                    .unwrap_or(name.to_str().context("Non-Unicode name")?.into()),
            );
            current = current.parent().context("Missing parent")?;
        }
        let mut final_path = PathBuf::from("/");
        for part in parts.into_iter().rev() {
            final_path.push(part);
        }
        e.final_path = path_string(&final_path)?;
        groups
            .entry(parent(&e.item.path).to_path_buf())
            .or_default()
            .push(index);
    }
    for (parent, indices) in groups {
        let selected: HashSet<String> = indices
            .iter()
            .map(|i| basename(&entries[*i].item.path).to_string())
            .collect();
        let mut existing = HashSet::new();
        for child in
            fs::read_dir(&parent).with_context(|| format!("Cannot list {}", parent.display()))?
        {
            let child = child?;
            let name = child.file_name();
            let name = name.to_str().context("Non-Unicode sibling name")?;
            if !selected.contains(name) {
                existing.insert(canonical(name));
            }
        }
        let mut desired: HashMap<String, usize> = HashMap::new();
        for i in indices {
            let key = canonical(&entries[i].name);
            if existing.contains(&key) {
                entries[i].error =
                    "Destination already exists (case/Unicode equivalent included)".into();
            }
            if let Some(other) = desired.insert(key, i) {
                entries[i].error = "Duplicate destination name".into();
                entries[other].error = "Duplicate destination name".into();
            }
        }
    }
    Ok(())
}
pub fn identity(path: &Path) -> Result<[u64; 2]> {
    let m = fs::symlink_metadata(path)?;
    Ok([m.dev(), m.ino()])
}
fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
pub fn rename_exclusive(source: &Path, destination: &Path) -> Result<()> {
    let src = CString::new(source.as_os_str().as_bytes())?;
    let dst = CString::new(destination.as_os_str().as_bytes())?;
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(src.as_ptr(), dst.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            src.as_ptr(),
            libc::AT_FDCWD,
            dst.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).with_context(|| {
            format!(
                "Cannot rename {} to {}",
                source.display(),
                destination.display()
            )
        });
    }
    Ok(())
}
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().context("Missing state directory")?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(temp.as_file_mut(), value)?;
    temp.as_file_mut().flush()?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    pub source: String,
    pub destination: String,
    pub identity: [u64; 2],
    pub state: String,
}
fn one() -> u32 {
    1
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Journal {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub batch_id: Option<String>,
    pub status: String,
    pub steps: Vec<Step>,
    pub entries: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
#[derive(Clone)]
pub struct Engine {
    pub root: PathBuf,
}
impl Engine {
    pub fn new(root: Option<PathBuf>) -> Result<Self> {
        let root = root.unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join("Library/Application Support/Renamer")
        });
        fs::create_dir_all(&root)?;
        Ok(Self {
            root: root.canonicalize()?,
        })
    }
    pub fn read(&self) -> Result<Option<Journal>> {
        let path = self.root.join("history.json");
        if !path.exists() {
            return Ok(None);
        }
        let record: Journal = serde_json::from_reader(File::open(path)?)?;
        if record.version != 1 {
            bail!("Unsupported history version; leave the journal intact");
        }
        Ok(Some(record))
    }
    fn save(&self, record: &Journal) -> Result<()> {
        save_json(&self.root.join("history.json"), record)
    }
    fn lock(&self) -> Result<File> {
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(self.root.join("execution.lock"))?;
        fs2::FileExt::try_lock_exclusive(&f)
            .context("Another Renamer instance is executing a batch")?;
        Ok(f)
    }
    pub fn execute(
        &self,
        entries: &mut [Entry],
        batch_id: &str,
        cancel: &AtomicBool,
        progress: impl Fn(usize, usize),
    ) -> Result<()> {
        let _lock = self.lock()?;
        let previous = self.read()?;
        if let Some(record) = &previous {
            if ["running", "reverting", "needs recovery"].contains(&record.status.as_str()) {
                bail!("Recover the unfinished batch before starting another");
            }
            if record.batch_id.as_deref() == Some(batch_id) {
                bail!("This batch has already been submitted; inspect history before retrying");
            }
        }
        validate(entries)?;
        if entries.iter().any(|e| !e.error.is_empty()) {
            bail!("Resolve all preview errors first");
        }
        for entry in entries.iter() {
            let current = Item::read(Path::new(&entry.item.path))?;
            if (current.identity, current.mtime, current.size)
                != (entry.item.identity, entry.item.mtime, entry.item.size)
            {
                bail!("An item changed since preview: {}", entry.item.path);
            }
            if entry.item.is_dir && self.root.starts_with(&entry.item.path) {
                bail!("State directory must be outside selected folders");
            }
        }
        let changed: Vec<Entry> = entries
            .iter()
            .filter(|e| basename(&e.item.path) != e.name)
            .cloned()
            .collect();
        if changed.is_empty() {
            return Ok(());
        }
        if let Some(record) = previous {
            save_json(&self.root.join(format!("history-{}.json", id())), &record)?;
        }
        let mut record = Journal {
            version: 1,
            batch_id: Some(batch_id.into()),
            status: "running".into(),
            steps: vec![],
            entries: entries.to_vec(),
            error: None,
        };
        self.save(&record)?;
        let result = (|| -> Result<()> {
            let mut groups: BTreeMap<PathBuf, Vec<Entry>> = BTreeMap::new();
            for e in &changed {
                groups
                    .entry(parent(&e.item.path).to_path_buf())
                    .or_default()
                    .push(e.clone());
            }
            let mut parents: Vec<_> = groups.keys().cloned().collect();
            parents.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
            let mut count = 0;
            for parent in parents {
                let mut staged = vec![];
                for e in &groups[&parent] {
                    stopped(cancel)?;
                    let temporary = parent.join(format!(".renamer-{}", id()));
                    self.step(
                        &mut record,
                        Path::new(&e.item.path),
                        &temporary,
                        e.item.identity,
                    )?;
                    staged.push((e, temporary));
                    count += 1;
                    progress(count, changed.len() * 2);
                }
                for (e, temporary) in staged {
                    stopped(cancel)?;
                    self.step(
                        &mut record,
                        &temporary,
                        &parent.join(&e.name),
                        e.item.identity,
                    )?;
                    count += 1;
                    progress(count, changed.len() * 2);
                }
            }
            record.status = "complete".into();
            self.save(&record)?;
            Ok(())
        })();
        if let Err(error) = result {
            if let Err(recovery) = self.reverse(&mut record, |_, _| {}) {
                bail!("{error:#}; recovery pending: {recovery:#}");
            }
            return Err(error.context("Completed operations were reverted"));
        }
        Ok(())
    }
    fn step(&self, record: &mut Journal, src: &Path, dst: &Path, expected: [u64; 2]) -> Result<()> {
        if identity(src)? != expected {
            bail!("Item identity changed: {}", src.display());
        }
        record.steps.push(Step {
            source: path_string(src)?,
            destination: path_string(dst)?,
            identity: expected,
            state: "pending".into(),
        });
        self.save(record)?;
        rename_exclusive(src, dst)?;
        record.steps.last_mut().unwrap().state = "done".into();
        self.save(record)
    }
    fn reverse(&self, record: &mut Journal, progress: impl Fn(usize, usize)) -> Result<()> {
        record.status = "reverting".into();
        self.save(record)?;
        let result = (|| -> Result<()> {
            for index in (0..record.steps.len()).rev() {
                let step = &record.steps[index];
                if step.state == "reverted" {
                    continue;
                }
                let src = Path::new(&step.source);
                let dst = Path::new(&step.destination);
                let src_id = if exists(src) {
                    Some(identity(src)?)
                } else {
                    None
                };
                let dst_id = if exists(dst) {
                    Some(identity(dst)?)
                } else {
                    None
                };
                if src_id == Some(step.identity) && dst_id != Some(step.identity) {
                } else if dst_id == Some(step.identity) && src_id.is_none() {
                    rename_exclusive(dst, src)?;
                } else {
                    bail!(
                        "Cannot safely restore {}. An item was moved or a destination is occupied",
                        step.source
                    );
                }
                record.steps[index].state = "reverted".into();
                self.save(record)?;
                progress(record.steps.len() - index, record.steps.len());
            }
            record.status = "undone".into();
            record.error = None;
            self.save(record)
        })();
        if let Err(error) = result {
            record.status = "needs recovery".into();
            record.error = Some(format!("{error:#}"));
            self.save(record)?;
            return Err(error);
        }
        Ok(())
    }
    pub fn undo(
        &self,
        expected: Option<&str>,
        progress: impl Fn(usize, usize),
    ) -> Result<Vec<String>> {
        let _lock = self.lock()?;
        let mut record = self.read()?.context("No batch to undo")?;
        if let Some(id) = expected
            && record.batch_id.as_deref() != Some(id)
        {
            bail!("Latest batch differs from the requested batch; inspect history");
        }
        if record.status == "undone" {
            bail!("No batch to undo");
        }
        if record.status == "complete" {
            let finals: HashSet<&str> = record
                .entries
                .iter()
                .map(|e| e.final_path.as_str())
                .collect();
            for e in &record.entries {
                if identity(Path::new(&e.final_path))? != e.item.identity {
                    bail!("An item was replaced or moved: {}", e.final_path);
                }
            }
            for e in &record.entries {
                let original = Path::new(&e.item.path);
                if exists(original)
                    && !finals.contains(e.item.path.as_str())
                    && identity(original)? != e.item.identity
                {
                    bail!("Original destination is occupied: {}", e.item.path);
                }
            }
        }
        self.reverse(&mut record, progress)?;
        Ok(record.entries.iter().map(|e| e.item.path.clone()).collect())
    }
}

/// Desktop folder selection: contents plus optional top-level folder candidates.
/// Keeps the CLI's explicit `contents: false` behavior unchanged.
pub fn scan_folder_selection(
    request: &Request,
    cancel: &AtomicBool,
) -> Result<(Vec<Item>, Vec<Item>)> {
    let items = scan(request, cancel)?;
    let roots = if request.contents {
        let mut roots_request = request.clone();
        roots_request.contents = false;
        scan(&roots_request, cancel)?
            .into_iter()
            .filter(|i| i.is_dir && !basename(&i.path).to_lowercase().ends_with(".app"))
            .collect()
    } else {
        vec![]
    };
    Ok((items, roots))
}
