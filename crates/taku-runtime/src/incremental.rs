//! The `unchanged` guard: fingerprint the task's inputs and skip the
//! remaining steps when nothing changed since the last successful run.

use std::collections::BTreeMap;
use std::path::PathBuf;

use mlua::{Table, Value};

use crate::error::Error;
use crate::exec::Ctx;

pub(crate) enum Decision {
    Skip,
    Run,
}

struct Fingerprint {
    files: u64,
    plan: u64,
    vars: u64,
    env: u64,
}

/// Stored raw: four little-endian u64 hashes, 32 bytes
impl Fingerprint {
    fn to_bytes(&self) -> [u8; 32] {
        let mut out = [0; 32];
        for (chunk, part) in out
            .chunks_exact_mut(8)
            .zip([self.files, self.plan, self.vars, self.env])
        {
            chunk.copy_from_slice(&part.to_le_bytes());
        }
        out
    }

    fn from_bytes(bytes: &[u8]) -> Option<Fingerprint> {
        let mut parts = bytes
            .chunks_exact(8)
            .map(|c| u64::from_le_bytes(c.try_into().unwrap()));
        (bytes.len() == 32).then(|| Fingerprint {
            files: parts.next().unwrap(),
            plan: parts.next().unwrap(),
            vars: parts.next().unwrap(),
            env: parts.next().unwrap(),
        })
    }

    /// The first differing part, for `--explain`.
    fn diff(&self, old: &Fingerprint) -> Option<&'static str> {
        [
            ("input files", self.files, old.files),
            ("step plan", self.plan, old.plan),
            ("vars", self.vars, old.vars),
            ("environment", self.env, old.env),
        ]
        .into_iter()
        .find(|(_, a, b)| a != b)
        .map(|(name, _, _)| name)
    }
}

/// Decides skip-or-run for an `unchanged` step. On `Run`, the new state is
/// left in `ctx.pending_state` for the executor to write after the task
/// succeeds; on `Skip` nothing is written.
pub(crate) fn check(spec: &Table, t: &Table, ctx: &mut Ctx) -> Result<Decision, Error> {
    let name: String = spec.get("name")?;
    let state_path = ctx.base.join(".taku/state").join(format!("{name}.bin"));
    let (fp, matched) = fingerprint(spec, t, ctx)?;

    let explain = |reason: &str| {
        if ctx.explain {
            crate::report::info(ctx.quiet, ctx.json, &format!("{name}: {reason}"));
        }
    };

    let rebuild = |reason: &str, ctx: &mut Ctx| {
        if ctx.explain {
            crate::report::info(ctx.quiet, ctx.json, &format!("{name}: rebuild ({reason})"));
        }
        ctx.pending_state = Some((state_path.clone(), fp.to_bytes()));
        Ok(Decision::Run)
    };

    if ctx.force {
        return rebuild("--force", ctx);
    }
    // No matching inputs means there is nothing to prove "unchanged" — the guard
    // must always run (and records no state, so it keeps running).
    if matched == 0 {
        explain("no matching inputs — always runs");
        return Ok(Decision::Run);
    }
    let old = match std::fs::read(&state_path) {
        Ok(bytes) => Fingerprint::from_bytes(&bytes),
        Err(_) => None,
    };
    let Some(old) = old else {
        return rebuild("no previous state", ctx);
    };
    if let Some(part) = fp.diff(&old) {
        return rebuild(&format!("{part} changed"), ctx);
    }
    if let Some(missing) = missing_output(t, ctx)? {
        return rebuild(&format!("output '{missing}' is missing"), ctx);
    }
    explain("skip (unchanged)");
    Ok(Decision::Skip)
}

/// `outputs = "path"` or `outputs = { "a", "b" }`
fn missing_output(t: &Table, ctx: &Ctx) -> Result<Option<String>, Error> {
    let outputs: Vec<String> = match t.get::<Value>("outputs")? {
        Value::Nil => Vec::new(),
        Value::String(s) => vec![s.to_string_lossy().to_string()],
        Value::Table(list) => list
            .sequence_values::<String>()
            .collect::<mlua::Result<_>>()?,
        other => {
            return Err(Error::TaskFailed(format!(
                "unchanged: 'outputs' must be a string or a list, got {}",
                other.type_name()
            )));
        }
    };
    for out in outputs {
        let path = ctx.format(&out)?;
        if !ctx.base.join(&path).exists() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// The task fingerprint and the number of input files the globs matched (zero
/// means there is nothing to compare, so the guard must never skip).
fn fingerprint(spec: &Table, t: &Table, ctx: &Ctx) -> Result<(Fingerprint, usize), Error> {
    let steps: Table = spec.get("steps")?;
    let mut plan_text = String::new();
    write_value(&mut plan_text, &Value::Table(steps), true);

    // Files enter the fingerprint by metadata (size + mtime), not content
    let mut files = String::new();
    let mut matched = 0usize;
    for pattern in t.sequence_values::<String>() {
        let pattern = ctx.format(&pattern?)?;
        let full = ctx.base.join(&pattern);
        let paths = glob::glob(&full.to_string_lossy()).map_err(|e| {
            Error::Task(Box::new(
                crate::diagnostic::Diagnostic::error(format!("invalid glob pattern '{pattern}'"))
                    .note(e.msg.to_string()),
            ))
        })?;
        let mut sorted: Vec<PathBuf> = paths.filter_map(Result::ok).collect();
        sorted.sort();
        let before = matched;
        for path in sorted {
            // Only regular files carry a size+mtime fingerprint — directories and
            // broken symlinks (metadata errors) contribute nothing.
            let Ok(meta) = path.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            files.push_str(&path.to_string_lossy());
            files.push_str(&format!("\n{},{mtime}\n", meta.len()));
            matched += 1;
        }
        // Warn when a pattern contributes no files at all — an empty glob, or one
        // that matched only directories: the guard can never call it unchanged.
        if matched == before && !ctx.quiet {
            let warning = crate::diagnostic::Diagnostic::warning(format!(
                "unchanged: glob '{pattern}' matched no files"
            ))
            .note("without matching inputs, this step is never considered unchanged");
            eprintln!(
                "{}",
                crate::diagnostic::renderer(ctx.json, crate::report::Style::init())
                    .render(&warning)
            );
        }
    }

    let mut vars = String::new();
    let mut sorted: Vec<_> = ctx.vars.iter().collect();
    sorted.sort();
    for (k, v) in sorted {
        vars.push_str(&format!("{k}={v}\n"));
    }

    // The whole environment, since the invoked tool can read the variables on its own.
    let mut all: BTreeMap<String, String> = std::env::vars_os()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .collect();
    for (k, v) in ctx.dotenv.iter() {
        all.entry(k.clone()).or_insert_with(|| v.clone());
    }
    let mut env = String::new();
    for (k, v) in all {
        env.push_str(&format!("{k}={v}\n"));
    }

    Ok((
        Fingerprint {
            files: hash(files.as_bytes()),
            plan: hash(plan_text.as_bytes()),
            vars: hash(vars.as_bytes()),
            env: hash(env.as_bytes()),
        },
        matched,
    ))
}

fn hash(data: &[u8]) -> u64 {
    xxhash_rust::xxh64::xxh64(data, 0)
}

/// Stable, order-independent text form of a step value. With `hash_fn`, a
/// function is fingerprinted by a hash of its (stripped) bytecode so an edited
/// body invalidates the cache; without it — for `--dry-run` display — it renders
/// as a plain `<function>`.
pub(crate) fn write_value(out: &mut String, v: &Value, hash_fn: bool) {
    match v {
        Value::String(s) => {
            out.push('"');
            out.push_str(&s.to_string_lossy());
            out.push('"');
        }
        Value::Integer(n) => out.push_str(&n.to_string()),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::Boolean(b) => out.push_str(&b.to_string()),
        Value::Function(f) if hash_fn => {
            // Stripped bytecode: the code, not line numbers, so an unrelated
            // edit above the function doesn't spuriously invalidate it.
            out.push_str(&format!("<function:{}>", hash(&f.dump(true))));
        }
        Value::Function(_) => out.push_str("<function>"),
        Value::Table(t) => {
            let mut pairs: Vec<(String, Value)> = Vec::new();
            for pair in t.pairs::<Value, Value>().flatten() {
                let mut key = String::new();
                write_value(&mut key, &pair.0, hash_fn);
                pairs.push((key, pair.1));
            }
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            out.push('{');
            for (k, v) in &pairs {
                out.push_str(k);
                out.push('=');
                write_value(out, v, hash_fn);
                out.push(';');
            }
            out.push('}');
        }
        other => out.push_str(other.type_name()),
    }
}
