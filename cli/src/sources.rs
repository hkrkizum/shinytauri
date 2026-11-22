//! Check unconditional local source() references without evaluating application code.
use crate::{Result, windows_names::name_key};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use tree_sitter::{Node, Parser};

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Encoding {
    Utf8,
    Latin1,
}

struct Include {
    path: String,
    line: usize,
    chdir: Option<bool>,
    encoding: Option<Encoding>,
    qualified: bool,
}

enum Step {
    Include(Include),
    UnknownDirectory,
    ShadowSource,
}

fn text<'a>(node: Node<'_>, code: &'a str) -> &'a str {
    &code[node.byte_range()]
}

// Only match an identifier or an explicit base namespace qualification. Member
// calls and strings that happen to contain the word "source" are unrelated.
fn base_function<'a>(node: Node<'_>, code: &'a str) -> Option<(&'a str, bool)> {
    let fun = node.child_by_field_name("function")?;
    match fun.kind() {
        "identifier" => Some((text(fun, code), false)),
        "namespace_operator" if text(fun.child_by_field_name("lhs")?, code) == "base" => {
            Some((text(fun.child_by_field_name("rhs")?, code), true))
        }
        _ => None,
    }
}

fn literal(node: Node<'_>, code: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let value = text(node.child_by_field_name("content")?, code);
    let open = text(node.child_by_field_name("open")?, code);
    if open != "'" && open != "\"" {
        return Some(value.to_owned()); // R raw string, with optional delimiter.
    }
    let mut decoded = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            decoded.push(c);
        } else {
            match chars.next()? {
                escaped @ ('\'' | '"' | '\\') => decoded.push(escaped),
                _ => return None,
            }
        }
    }
    Some(decoded)
}

fn source_call(node: Node<'_>, code: &str, qualified: bool) -> Option<Include> {
    let args = node.child_by_field_name("arguments")?;
    // R matches exact names first, then unique prefixes, then the remaining
    // unnamed arguments in formal order. Never treat a dynamic expression as
    // evidence of a concrete path or working directory.
    const FORMALS: [&str; 17] = [
        "file",
        "local",
        "echo",
        "print.eval",
        "exprs",
        "spaced",
        "verbose",
        "prompt.echo",
        "max.deparse.length",
        "width.cutoff",
        "deparseCtrl",
        "chdir",
        "catch.aborts",
        "encoding",
        "continue.echo",
        "skip.echo",
        "keep.source",
    ];
    let mut bound = [None; FORMALS.len()];
    let mut partial = Vec::new();
    let mut unnamed = Vec::new();
    for arg in args.children_by_field_name("argument", &mut args.walk()) {
        let value = arg.child_by_field_name("value")?;
        if value.kind() == "dots" {
            return None;
        }
        if let Some(key) = arg.child_by_field_name("name") {
            let name = text(key, code).trim_matches(['\'', '"', '`']);
            if let Some(index) = FORMALS.iter().position(|&f| f == name) {
                if bound[index].replace(value).is_some() {
                    return None;
                }
            } else {
                partial.push((name, value));
            }
        } else {
            unnamed.push(value);
        }
    }
    for (name, value) in partial {
        let mut matching = FORMALS
            .iter()
            .enumerate()
            .filter(|(i, f)| bound[*i].is_none() && f.starts_with(name));
        let (index, _) = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        bound[index] = Some(value);
    }
    let mut free = bound.iter_mut().filter(|slot| slot.is_none());
    for value in unnamed {
        *free.next()? = Some(value);
    }
    let get = |name| bound[FORMALS.iter().position(|&f| f == name).unwrap()];
    let chdir = match get("chdir").map(|n| text(n, code)) {
        None | Some("FALSE") => Some(false),
        Some("TRUE") => Some(true),
        _ => None,
    };
    let encoding = match get("encoding") {
        None => Some(Encoding::Utf8),
        Some(value) => match literal(value, code)
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("utf-8" | "utf8") => Some(Encoding::Utf8),
            Some("latin1" | "iso-8859-1") => Some(Encoding::Latin1),
            _ => None,
        },
    };
    Some(Include {
        path: literal(get("file")?, code)?,
        line: node.start_position().row + 1,
        chdir,
        encoding,
        qualified,
    })
}

fn steps(node: Node<'_>, code: &str, result: &mut Vec<Step>) {
    match node.kind() {
        "program" | "braced_expression" | "parenthesized_expression" => {
            for child in node.named_children(&mut node.walk()) {
                steps(child, code, result);
            }
        }
        "binary_operator" => {
            let Some(op) = node.child_by_field_name("operator") else {
                return;
            };
            if matches!(text(op, code), "<-" | "<<-" | "=") {
                if let Some(rhs) = node.child_by_field_name("rhs") {
                    steps(rhs, code, result);
                }
                if node
                    .child_by_field_name("lhs")
                    .is_some_and(|n| text(n, code) == "source")
                {
                    result.push(Step::ShadowSource);
                }
            }
        }
        "call" => match base_function(node, code) {
            Some(("source", qualified)) => {
                if let Some(call) = source_call(node, code, qualified) {
                    result.push(Step::Include(call));
                }
            }
            Some(("setwd", _)) => result.push(Step::UnknownDirectory),
            _ => {}
        },
        "if_statement" | "for_statement" | "while_statement" | "repeat_statement" => {
            uncertain_effects(node, code, result);
        }
        _ => {}
    }
}

// A branch or loop may change how later expressions are resolved. Record that
// uncertainty without interpreting any source() in the branch.
fn uncertain_effects(node: Node<'_>, code: &str, result: &mut Vec<Step>) {
    if node.kind() == "function_definition"
        || matches!(
            base_function(node, code),
            Some(("quote" | "substitute" | "expression" | "alist", _))
        )
    {
        return;
    }
    if matches!(base_function(node, code), Some(("setwd", _))) {
        result.push(Step::UnknownDirectory);
    }
    if node.kind() == "binary_operator"
        && node
            .child_by_field_name("operator")
            .is_some_and(|n| matches!(text(n, code), "<-" | "<<-" | "="))
        && node
            .child_by_field_name("lhs")
            .is_some_and(|n| text(n, code) == "source")
    {
        result.push(Step::ShadowSource);
    }
    for child in node.named_children(&mut node.walk()) {
        uncertain_effects(child, code, result);
    }
}

// Keep lexical '..' only when it resolves inside the app. A Windows drive,
// URL, UNC path or home-relative file is outside the local bundle contract.
fn resolve(directory: &str, path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    if path.is_empty() || path.starts_with(['/', '~']) || path.contains(':') {
        return None;
    }
    let mut parts: Vec<_> = directory.split('/').filter(|s| !s.is_empty()).collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[derive(Clone, Copy, Default)]
struct Effects {
    changed_directory: bool,
    shadowed_source: bool,
}

// Use the selected Windows spelling rules on a case-sensitive build computer.
// Do not follow unselected links while checking whether a local file exists.
fn original_file(app: &Path, relative: &str) -> Result<Option<String>> {
    let mut actual = Vec::new();
    let mut dir = app.to_owned();
    for part in relative.split('/') {
        if !dir.is_dir() {
            return Ok(None);
        }
        let mut matched = None;
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name_key(name) == name_key(part) {
                matched = Some(name.to_owned());
                if name == part {
                    break;
                }
            }
        }
        let Some(name) = matched else { return Ok(None) };
        dir.push(&name);
        if crate::inspect::is_link(&fs::symlink_metadata(&dir)?) {
            return Ok(None);
        }
        actual.push(name);
    }
    Ok(dir.is_file().then(|| actual.join("/")))
}

struct Checker<'a> {
    app: &'a Path,
    original: &'a Path,
    files: BTreeMap<String, &'a str>,
    parser: Parser,
    visited: BTreeMap<(String, String, Encoding, bool), Effects>,
    missing: BTreeSet<String>,
    unknown: BTreeSet<String>,
}

impl Checker<'_> {
    fn follow(
        &mut self,
        file: &str,
        directory: &str,
        encoding: Encoding,
        shadowed: bool,
    ) -> Result<Effects> {
        let key = (file.to_owned(), directory.to_owned(), encoding, shadowed);
        let mut effects = Effects {
            shadowed_source: shadowed,
            ..Effects::default()
        };
        if let Some(known) = self.visited.get(&key) {
            return Ok(*known);
        }
        self.visited.insert(key.clone(), effects);
        let bytes = fs::read(self.app.join(file))?;
        let code = match encoding {
            Encoding::Latin1 => bytes.iter().map(|b| char::from(*b)).collect(),
            Encoding::Utf8 => match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => return Ok(effects),
            },
        };
        let Some(tree) = self.parser.parse(code.as_bytes(), None) else {
            return Ok(effects);
        };
        if tree.root_node().has_error() {
            return Ok(effects);
        }
        let mut calls = Vec::new();
        steps(tree.root_node(), &code, &mut calls);

        for step in calls {
            match step {
                Step::UnknownDirectory => {
                    effects.changed_directory = true;
                    break;
                }
                Step::ShadowSource => effects.shadowed_source = true,
                Step::Include(call) if call.qualified || !effects.shadowed_source => {
                    let Some(path) = resolve(directory, &call.path) else {
                        continue;
                    };
                    if let Some(&actual) = self.files.get(&name_key(&path)) {
                        if let (Some(chdir), Some(enc)) = (call.chdir, call.encoding) {
                            let cwd = if chdir {
                                actual.rsplit_once('/').map_or("", |(p, _)| p)
                            } else {
                                directory
                            };
                            let child = self.follow(actual, cwd, enc, effects.shadowed_source)?;
                            effects.shadowed_source |= child.shadowed_source;
                            if !chdir && child.changed_directory {
                                effects.changed_directory = true;
                                break;
                            }
                        }
                    } else {
                        let message =
                            format!("{}:{} refers to {path:?}", file.escape_debug(), call.line);
                        if let Some(actual) = original_file(self.original, &path)? {
                            self.missing
                                .insert(format!("{message} (include {actual:?})"));
                        } else {
                            self.unknown.insert(message);
                        }
                    }
                }
                Step::Include(_) => {}
            }
        }
        self.visited.insert(key, effects);
        Ok(effects)
    }
}

pub(crate) fn check(app: &Path, original: &Path, files: &BTreeSet<String>) -> Result<()> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_r::LANGUAGE.into())?;
    let mut checker = Checker {
        app,
        original,
        files: files.iter().map(|f| (name_key(f), f.as_str())).collect(),
        parser,
        visited: BTreeMap::new(),
        missing: BTreeSet::new(),
        unknown: BTreeSet::new(),
    };
    let split = checker.files.contains_key("SERVER.R");
    let mut roots = Vec::new();
    if split {
        roots.extend(checker.files.get("GLOBAL.R").copied());
    }
    if !checker.files.contains_key("R/_DISABLE_AUTOLOAD.R") {
        roots.extend(
            files
                .iter()
                .filter(|f| {
                    f.split_once('/').is_some_and(|(dir, name)| {
                        dir.eq_ignore_ascii_case("R")
                            && !name.starts_with('.')
                            && !name.contains('/')
                            && name.to_ascii_lowercase().ends_with(".r")
                    })
                })
                .map(String::as_str),
        );
    }
    for entry in if split {
        &["UI.R", "SERVER.R"][..]
    } else {
        &["APP.R"][..]
    } {
        roots.extend(checker.files.get(*entry).copied());
    }
    let mut shadowed = false;
    for file in roots {
        shadowed |= checker
            .follow(file, "", Encoding::Utf8, shadowed)?
            .shadowed_source;
    }
    for entry in checker.unknown {
        eprintln!(
            "Warning: {entry}, absent from the app directory. Check the path if it is not generated at runtime."
        );
    }
    if !checker.missing.is_empty() {
        return Err(format!("Local source files are missing from the bundle. Add these paths to files in the bundle config:\n  {}",
            checker.missing.into_iter().collect::<Vec<_>>().join("\n  ")).into());
    }
    Ok(())
}
