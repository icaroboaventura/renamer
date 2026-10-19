use anyhow::{Context, Result, bail};
use fancy_regex::{Captures, Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use unicode_categories::UnicodeCategories;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

fn yes() -> bool {
    true
}
fn name_target() -> String {
    "Name".into()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub kind: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "name_target")]
    pub target: String,
    #[serde(default = "empty_options")]
    pub options: Value,
}
fn empty_options() -> Value {
    json!({})
}
impl Rule {
    pub fn new(kind: &str) -> Self {
        let mode = modes(kind).first().cloned();
        let mut rule = Self {
            kind: kind.into(),
            enabled: true,
            target: name_target(),
            options: json!({}),
        };
        if let Some(mode) = mode {
            rule.options["mode"] = json!(mode);
        }
        rule
    }
    pub fn s<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.options
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(default)
    }
    pub fn b(&self, key: &str) -> bool {
        self.options
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }
    pub fn n(&self, key: &str, default: i64) -> i64 {
        self.options
            .get(key)
            .and_then(Value::as_i64)
            .unwrap_or(default)
    }
    pub fn validate(&self) -> Result<()> {
        let c = catalog();
        let operation = c["operations"]
            .get(&self.kind)
            .context("Unknown rule kind")?;
        if !["Name", "Extension", "Full name"].contains(&self.target.as_str()) {
            bail!("Invalid rule target");
        }
        let options = self
            .options
            .as_object()
            .context("Rule options must be an object")?;
        for (key, v) in options {
            if !operation["options"]
                .as_array()
                .unwrap()
                .contains(&json!(key))
            {
                bail!("Unknown option {key} for {}", self.kind);
            }
            match key.as_str() {
                "first" | "ignore_case" | "split_numbers" | "per_folder" => {
                    if !v.is_boolean() {
                        bail!("{key} must be boolean");
                    }
                }
                "position" | "start" | "step" | "digits" | "count" => {
                    if v.as_i64().is_none() {
                        bail!("{key} must be an integer");
                    }
                }
                _ => {
                    if !v.is_string() {
                        bail!("{key} must be a string");
                    }
                }
            }
        }
        if options.contains_key("mode")
            && !modes(&self.kind).contains(&self.s("mode", "").to_string())
        {
            bail!("Invalid mode for {}", self.kind);
        }
        if !(1..=20).contains(&self.n("digits", 3)) {
            bail!("digits must be between 1 and 20");
        }
        if self.n("count", 1) < 0 {
            bail!("count cannot be negative");
        }
        Ok(())
    }
}
pub fn catalog() -> Value {
    let mut value: Value = serde_json::from_str(include_str!("../assets/catalog.json")).unwrap();
    value["implementation"] = json!(
        "Rust 2.0; fancy-regex with Python-style replacements, isolated worker and backtracking limits"
    );
    value
}
pub fn kinds() -> Vec<String> {
    vec![
        "Case",
        "Word format",
        "Replace text",
        "Regular expression",
        "Remove",
        "Whitespace",
        "Insert",
        "Numbering",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}
pub fn modes(kind: &str) -> Vec<String> {
    catalog()["operations"][kind]["modes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Debug)]
enum Replacement {
    Literal(String),
    Index(usize),
    Named(String),
}
fn replacement_tokens(text: &str, regex: &Regex) -> Result<Vec<Replacement>> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut literal = String::new();
    let mut tokens = vec![];
    while i < chars.len() {
        if chars[i] != '\\' {
            literal.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i == chars.len() {
            bail!("Trailing backslash in replacement");
        }
        let c = chars[i];
        i += 1;
        let mut group = None;
        if c == 'g' {
            if chars.get(i) != Some(&'<') {
                bail!("Use \\g<1> or \\g<name> for a capture reference");
            }
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != '>' {
                i += 1;
            }
            if i == chars.len() {
                bail!("Unclosed capture reference");
            }
            group = Some(chars[start..i].iter().collect::<String>());
            i += 1;
        } else if c.is_ascii_digit() {
            let octal = c == '0'
                || (('0'..='7').contains(&c)
                    && chars.get(i).is_some_and(|c| ('0'..='7').contains(c))
                    && chars.get(i + 1).is_some_and(|c| ('0'..='7').contains(c)));
            if octal {
                let mut digits = c.to_string();
                while digits.len() < 3 && chars.get(i).is_some_and(|c| ('0'..='7').contains(c)) {
                    digits.push(chars[i]);
                    i += 1;
                }
                let number = u32::from_str_radix(&digits, 8)?;
                if number > 255 {
                    bail!("Octal replacement escape is outside 0o377");
                }
                literal.push(char::from_u32(number).unwrap());
            } else {
                let mut digits = c.to_string();
                if chars.get(i).is_some_and(char::is_ascii_digit) {
                    digits.push(chars[i]);
                    i += 1;
                }
                group = Some(digits);
            }
        } else {
            match c {
                '\\' => literal.push('\\'),
                'n' => literal.push('\n'),
                'r' => literal.push('\r'),
                't' => literal.push('\t'),
                'a' => literal.push('\x07'),
                'b' => literal.push('\x08'),
                'f' => literal.push('\x0c'),
                'v' => literal.push('\x0b'),
                c if c.is_ascii_alphabetic() => bail!("Unsupported replacement escape \\{c}"),
                _ => {
                    literal.push('\\');
                    literal.push(c);
                }
            }
        }
        if let Some(group) = group {
            if !literal.is_empty() {
                tokens.push(Replacement::Literal(std::mem::take(&mut literal)));
            }
            if let Ok(index) = group.parse::<usize>() {
                if index >= regex.captures_len() {
                    bail!("Invalid capture group {index}");
                }
                tokens.push(Replacement::Index(index));
            } else {
                if !regex.capture_names().flatten().any(|n| n == group) {
                    bail!("Unknown capture group {group}");
                }
                tokens.push(Replacement::Named(group));
            }
        }
    }
    if !literal.is_empty() {
        tokens.push(Replacement::Literal(literal));
    }
    Ok(tokens)
}
fn expand(tokens: &[Replacement], caps: &Captures<'_>) -> String {
    tokens
        .iter()
        .map(|t| match t {
            Replacement::Literal(s) => s.clone(),
            Replacement::Index(n) => caps.get(*n).map(|m| m.as_str()).unwrap_or("").into(),
            Replacement::Named(s) => caps.name(s).map(|m| m.as_str()).unwrap_or("").into(),
        })
        .collect()
}

pub struct CompiledRule {
    pub rule: Rule,
    regex: Option<Regex>,
    replacement: Vec<Replacement>,
}
impl CompiledRule {
    pub fn new(rule: Rule) -> Result<Self> {
        rule.validate()?;
        let mut result = Self {
            rule,
            regex: None,
            replacement: vec![],
        };
        if ["Regular expression", "Replace text"].contains(&result.rule.kind.as_str()) {
            let raw = result.rule.s("pattern", "");
            let pattern = if result.rule.kind == "Replace text" {
                regex::escape(raw)
            } else {
                python_pattern(raw)?
            };
            let regex = RegexBuilder::new(&pattern)
                .case_insensitive(result.rule.b("ignore_case"))
                .backtrack_limit(100_000)
                .delegate_size_limit(4_000_000)
                .build()
                .context("Invalid regex")?;
            result.replacement = if result.rule.kind == "Replace text" {
                vec![Replacement::Literal(
                    result.rule.s("replacement", "").into(),
                )]
            } else {
                replacement_tokens(result.rule.s("replacement", ""), &regex)?
            };
            result.regex = Some(regex);
        }
        Ok(result)
    }
    pub fn transform(&self, text: &str, index: usize) -> Result<String> {
        let r = &self.rule;
        let mode = r.s("mode", "");
        Ok(match r.kind.as_str() {
            "Case" => match mode {
                "UPPERCASE" => text.to_uppercase(),
                "Title Case" => title(text),
                "Sentence case" => capitalize(text),
                "Invert case" => text
                    .chars()
                    .map(|c| {
                        if c.is_uppercase() {
                            c.to_lowercase().collect::<String>()
                        } else {
                            c.to_uppercase().collect()
                        }
                    })
                    .collect(),
                _ => text.to_lowercase(),
            },
            "Word format" => {
                let words = words(text, r.b("split_numbers"));
                let lower: Vec<_> = words.iter().map(|w| w.to_lowercase()).collect();
                let titled: Vec<_> = words.iter().map(|w| capitalize(w)).collect();
                match mode {
                    "camelCase" => {
                        lower.first().cloned().unwrap_or_default()
                            + &titled.iter().skip(1).cloned().collect::<String>()
                    }
                    "PascalCase" => titled.concat(),
                    "SCREAMING_SNAKE_CASE" => lower.join("_").to_uppercase(),
                    "kebab-case" => lower.join("-"),
                    "Train-Case" => titled.join("-"),
                    "Separate words" => words.join(" "),
                    "Join words" => words.concat(),
                    _ => lower.join("_"),
                }
            }
            "Replace text" if r.s("pattern", "").is_empty() => text.into(),
            "Replace text" | "Regular expression" => self.replace(text)?.0,
            "Remove" => match mode {
                "Characters at position" => {
                    let v: Vec<char> = text.chars().collect();
                    let p = position(r.n("position", 0), v.len());
                    v[..p]
                        .iter()
                        .chain(v[p.saturating_add(r.n("count", 1) as usize).min(v.len())..].iter())
                        .collect()
                }
                "Digits" => text.chars().filter(|c| !c.is_numeric()).collect(),
                "Punctuation" => text.chars().filter(|c| !c.is_punctuation()).collect(),
                "Accents" => text.nfd().filter(|c| !is_combining_mark(*c)).collect(),
                _ => text.replace(r.s("text", ""), ""),
            },
            "Whitespace" => match mode {
                "Remove all spaces" => text.chars().filter(|c| !c.is_whitespace()).collect(),
                "Trim ends" => text.trim().into(),
                "Collapse spaces" => space_replace(text, " "),
                "Spaces to separator" => space_replace(text, r.s("separator", "-")),
                "Hyphens to spaces" => text.replace('-', " "),
                _ => text.replace('_', " "),
            },
            "Insert" => {
                let value = r.s("text", "");
                match mode {
                    "Replace entire name" => value.into(),
                    "Prefix" => format!("{value}{text}"),
                    "Suffix" => format!("{text}{value}"),
                    _ => insert(text, value, r.n("position", 0)),
                }
            }
            "Numbering" => {
                let number = (r.n("start", 1) as i128) + (index as i128) * (r.n("step", 1) as i128);
                let number = format!("{:0width$}", number, width = r.n("digits", 3) as usize);
                let sep = r.s("separator", "_");
                match mode {
                    "Replace entire name" => number,
                    "Prefix" => format!("{number}{sep}{text}"),
                    "At position" => {
                        insert(text, &format!("{sep}{number}{sep}"), r.n("position", 0))
                    }
                    _ => format!("{text}{sep}{number}"),
                }
            }
            _ => bail!("Unknown operation"),
        })
    }
    pub fn replace(&self, text: &str) -> Result<(String, Vec<[usize; 2]>)> {
        let regex = self.regex.as_ref().context("Not a regex rule")?;
        let mut output = String::new();
        let mut previous = 0;
        let mut spans = vec![];
        for caps in regex.captures_iter(text) {
            let caps =
                caps.context("Regex exceeded its backtracking limit; simplify the pattern")?;
            let m = caps.get(0).unwrap();
            output.push_str(&text[previous..m.start()]);
            output.push_str(&expand(&self.replacement, &caps));
            previous = m.end();
            spans.push([
                text[..m.start()].chars().count(),
                text[..m.end()].chars().count(),
            ]);
            if self.rule.b("first") {
                break;
            }
        }
        output.push_str(&text[previous..]);
        Ok((output, spans))
    }
}
fn python_pattern(pattern: &str) -> Result<String> {
    if pattern.len() > 65536 {
        bail!("Regex is too long");
    }
    // Python end-of-string is \Z, while this engine calls it \z. Preserve escaped backslashes.
    let mut result = String::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        result.push(c);
        if c == '\\'
            && let Some(next) = chars.next()
        {
            result.push(if next == 'Z' { 'z' } else { next });
        }
    }
    Ok(result)
}
fn position(p: i64, len: usize) -> usize {
    if p < 0 {
        (len as i128 + p as i128).max(0) as usize
    } else {
        (p as usize).min(len)
    }
}
fn insert(text: &str, value: &str, p: i64) -> String {
    let v: Vec<char> = text.chars().collect();
    let p = position(p, v.len());
    format!(
        "{}{}{}",
        v[..p].iter().collect::<String>(),
        value,
        v[p..].iter().collect::<String>()
    )
}
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}
fn title(s: &str) -> String {
    let mut previous = false;
    s.chars()
        .map(|c| {
            let value = if previous {
                c.to_lowercase().collect::<String>()
            } else {
                c.to_uppercase().collect()
            };
            previous = c.is_lowercase() || c.is_uppercase();
            value
        })
        .collect()
}
fn space_replace(s: &str, replacement: &str) -> String {
    let mut output = String::new();
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !space {
                output.push_str(replacement);
            }
            space = true;
        } else {
            output.push(c);
            space = false;
        }
    }
    output
}
pub fn words(text: &str, numbers: bool) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut words = vec![];
    let mut word = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            continue;
        }
        if i > 0 && !word.is_empty() {
            let prev = chars[i - 1];
            let next = chars.get(i + 1).copied().unwrap_or(' ');
            if (prev.is_lowercase() && c.is_uppercase())
                || (prev.is_uppercase() && c.is_uppercase() && next.is_lowercase())
                || (numbers
                    && ((prev.is_alphabetic() && c.is_numeric())
                        || (prev.is_numeric() && c.is_alphabetic())))
            {
                words.push(std::mem::take(&mut word));
            }
        }
        word.push(c);
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}
pub fn split_name(name: &str, is_dir: bool, compound: bool) -> (&str, &str) {
    if is_dir || !name.trim_start_matches('.').contains('.') {
        return (name, "");
    }
    if compound {
        for ext in [
            ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".user.js", ".d.ts",
        ] {
            if name.to_lowercase().ends_with(ext) {
                return name.split_at(name.len() - ext.len());
            }
        }
    }
    name.rfind('.')
        .map(|p| name.split_at(p))
        .unwrap_or((name, ""))
}
pub fn apply(
    name: &str,
    is_dir: bool,
    rules: &[CompiledRule],
    index: usize,
    folder_index: usize,
    compound: bool,
) -> Result<String> {
    let mut name = name.to_string();
    for compiled in rules {
        let r = &compiled.rule;
        if !r.enabled {
            continue;
        }
        let i = if r.b("per_folder") {
            folder_index
        } else {
            index
        };
        let (stem, ext) = split_name(&name, is_dir, compound);
        name = match r.target.as_str() {
            "Full name" => compiled.transform(&name, i)?,
            "Extension" if !is_dir => {
                let ext = compiled.transform(ext.trim_start_matches('.'), i)?;
                format!("{stem}{}{}", if ext.is_empty() { "" } else { "." }, ext)
            }
            "Extension" => name.clone(),
            _ => compiled.transform(stem, i)? + ext,
        };
    }
    Ok(name)
}
