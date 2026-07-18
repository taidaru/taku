//! Task-header parsing and `---` doc-comment scanning.

use std::collections::HashMap;

pub(crate) struct Param {
    pub name: String,
    pub default: Option<String>,
}

pub(crate) struct Header {
    pub name: String,
    pub params: Vec<Param>,
    pub deps: Vec<String>,
}

/// Parses `"name <p1> <p2=def>: dep1 dep2"`. Params and deps are optional.
pub(crate) fn parse_header(header: &str) -> Result<Header, String> {
    let (left, deps_part) = match header.split_once(':') {
        Some((l, r)) => (l, Some(r)),
        None => (header, None),
    };

    let mut tokens = left.split_whitespace();
    let name = tokens
        .next()
        .ok_or_else(|| format!("task header '{header}': missing task name"))?;
    if name.contains('<') || name.contains('>') {
        return Err(format!(
            "task header '{header}': the task name must come before any <param>"
        ));
    }

    let mut params = Vec::new();
    for tok in tokens {
        let inner = tok
            .strip_prefix('<')
            .and_then(|t| t.strip_suffix('>'))
            .ok_or_else(|| {
                format!("task header '{header}': expected <param> or <param=default>, got '{tok}'")
            })?;
        let (pname, default) = match inner.split_once('=') {
            Some((n, d)) => (n, Some(d.to_string())),
            None => (inner, None),
        };
        if pname.is_empty() {
            return Err(format!("task header '{header}': empty param name"));
        }
        params.push(Param {
            name: pname.to_string(),
            default,
        });
    }

    let deps = deps_part
        .map(|d| d.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();

    Ok(Header {
        name: name.to_string(),
        params,
        deps,
    })
}

/// A block of `---` lines directly above a
/// `task("name ...")` line documents that task. The name is taken from the
/// first string literal on the `task(` line (header part before `<`/`:`).
/// Lua discards comments, so this scans the source text instead.
pub(crate) fn scan_docs(source: &str, docs: &mut HashMap<String, String>) {
    let mut block: Vec<&str> = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if let Some(text) = trimmed.strip_prefix("---") {
            block.push(text.trim());
            continue;
        }
        if !block.is_empty()
            && let Some(header) = task_header_literal(trimmed)
        {
            let name = header
                .split(['<', ':'])
                .next()
                .unwrap_or(&header)
                .trim()
                .to_string();
            if !name.is_empty() {
                docs.insert(name, block.join("\n"));
            }
        }
        block.clear();
    }
}

fn task_header_literal(line: &str) -> Option<String> {
    let rest = line.strip_prefix("task")?;
    let rest = rest
        .trim_start()
        .strip_prefix('(')
        .unwrap_or(rest)
        .trim_start();
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let body = &rest[1..];
    body.find(quote).map(|end| body[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_bare_name() {
        let h = parse_header("build").unwrap();
        assert_eq!(h.name, "build");
        assert!(h.params.is_empty() && h.deps.is_empty());
    }

    #[test]
    fn parses_params_and_deps() {
        let h = parse_header("deploy <env> <region=eu>: build test").unwrap();
        assert_eq!(h.name, "deploy");
        assert_eq!(h.params.len(), 2);
        assert_eq!(h.params[0].name, "env");
        assert_eq!(h.params[0].default, None);
        assert_eq!(h.params[1].name, "region");
        assert_eq!(h.params[1].default.as_deref(), Some("eu"));
        assert_eq!(h.deps, ["build", "test"]);
    }

    #[test]
    fn deps_only_header_works() {
        let h = parse_header("dev: api web").unwrap();
        assert_eq!(h.name, "dev");
        assert_eq!(h.deps, ["api", "web"]);
    }

    #[test]
    fn malformed_headers_are_errors() {
        assert!(parse_header("").is_err());
        assert!(parse_header("name stray-token").is_err());
        assert!(parse_header("name <>").is_err());
        assert!(parse_header("<p> name").is_err());
    }

    #[test]
    fn docs_attach_to_the_task_below() {
        let src = "\
--- build the project
--- second line
task(\"build: deps\", {})

task('undocumented', {})

--- stale block broken by a blank line

task('blank-separated', {})
";
        let mut docs = HashMap::new();
        scan_docs(src, &mut docs);
        assert_eq!(docs.get("build").unwrap(), "build the project\nsecond line");
        assert!(!docs.contains_key("undocumented"));
        assert!(!docs.contains_key("blank-separated"));
    }

    #[test]
    fn doc_name_strips_params_and_deps() {
        let mut docs = HashMap::new();
        scan_docs("--- doc\ntask(\"deploy <env>: build\", {})\n", &mut docs);
        assert_eq!(docs.get("deploy").unwrap(), "doc");
    }
}
