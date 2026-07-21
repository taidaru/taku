use std::collections::{HashMap, HashSet};
use std::path::Path;

use mlua::{Lua, Table};

use crate::error::Error;
use crate::state::TASKS_KEY;

#[derive(Debug)]
pub(crate) struct Plan {
    pub tasks: Vec<String>,
    pub deps: HashMap<String, Vec<String>>,
}

pub(crate) fn build(lua: &Lua, takufile: &Path, command: &str) -> Result<Plan, Error> {
    let tasks: Table = lua.named_registry_value(TASKS_KEY)?;
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    let mut stack: Vec<String> = Vec::new();
    collect(&tasks, takufile, command, &mut deps, &mut stack)?;

    let mut names: Vec<String> = deps.keys().cloned().collect();
    names.sort();
    Ok(Plan { tasks: names, deps })
}

/// Pretty execution graph.
pub(crate) fn render(plan: &Plan, root: &str) -> String {
    fn walk(
        name: &str,
        prefix: &str,
        deps: &HashMap<String, Vec<String>>,
        seen: &mut HashSet<String>,
        out: &mut String,
    ) {
        let ds = deps.get(name).map(Vec::as_slice).unwrap_or_default();
        for (i, dep) in ds.iter().enumerate() {
            let last = i + 1 == ds.len();
            out.push_str(prefix);
            out.push_str(if last { "└─ " } else { "├─ " });
            out.push_str(dep);
            // Expand each shared subtree only once; a repeat of a node that has
            // children is shown as `name …`, so a diamond or duplicate-dep
            // graph renders in O(nodes) instead of blowing up exponentially.
            let child_prefix = format!("{prefix}{}", if last { "   " } else { "│  " });
            if seen.insert(dep.clone()) {
                out.push('\n');
                walk(dep, &child_prefix, deps, seen, out);
            } else if deps.get(dep).is_some_and(|d| !d.is_empty()) {
                out.push_str(" …\n");
            } else {
                out.push('\n');
            }
        }
    }

    let mut out = format!("{root}\n");
    let mut seen = HashSet::from([root.to_string()]);
    walk(root, "", &plan.deps, &mut seen, &mut out);
    out
}

fn collect(
    tasks: &Table,
    takufile: &Path,
    name: &str,
    deps: &mut HashMap<String, Vec<String>>,
    stack: &mut Vec<String>,
) -> Result<(), Error> {
    if deps.contains_key(name) {
        return Ok(());
    }

    if stack.iter().any(|n| n == name) {
        let mut cycle = stack.clone();
        cycle.push(name.to_string());
        return Err(Error::DependencyCycle(cycle));
    }

    let spec: Table = match tasks.get(name)? {
        Some(spec) => spec,
        None => {
            return Err(Error::UnknownCommand {
                name: name.to_string(),
                takufile: takufile.to_path_buf(),
                available: available_commands(tasks),
            });
        }
    };

    stack.push(name.to_string());
    let dep_table: Table = spec.get("deps")?;
    let mut dep_names = Vec::new();
    for dep in dep_table.sequence_values::<String>() {
        let dep = dep?;
        collect(tasks, takufile, &dep, deps, stack)?;
        // Dedup a task's own repeated deps so the ready-queue and the rendered
        // tree don't process the same edge twice.
        if !dep_names.contains(&dep) {
            dep_names.push(dep);
        }
    }
    stack.pop();
    deps.insert(name.to_string(), dep_names);
    Ok(())
}

fn available_commands(tasks: &Table) -> Vec<String> {
    let mut names: Vec<String> = tasks
        .pairs::<String, Table>()
        .filter_map(Result::ok)
        .map(|(name, _)| name)
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;

    fn lua_with_tasks(tasks: &[(&str, &[&str])]) -> Lua {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        for (name, deps) in tasks {
            let spec = lua.create_table().unwrap();
            spec.set("deps", lua.create_sequence_from(deps.to_vec()).unwrap())
                .unwrap();
            table.set(*name, spec).unwrap();
        }
        lua.set_named_registry_value(TASKS_KEY, &table).unwrap();
        lua
    }

    fn build_for(tasks: &[(&str, &[&str])], command: &str) -> Result<Plan, Error> {
        let lua = lua_with_tasks(tasks);
        build(&lua, Path::new("Takufile.lua"), command)
    }

    #[test]
    fn collects_transitive_deps_of_a_diamond() {
        let plan = build_for(
            &[("a", &["b", "c"]), ("b", &["d"]), ("c", &["d"]), ("d", &[])],
            "a",
        )
        .unwrap();
        assert_eq!(plan.tasks, ["a", "b", "c", "d"]);
        assert_eq!(plan.deps["a"], ["b", "c"]);
        assert_eq!(plan.deps["d"], [] as [String; 0]);
    }

    #[test]
    fn ignores_tasks_outside_the_requested_subgraph() {
        let plan = build_for(&[("a", &["b"]), ("b", &[]), ("unrelated", &[])], "a").unwrap();
        assert_eq!(plan.tasks, ["a", "b"]);
    }

    #[test]
    fn render_draws_a_tree_from_the_target() {
        let plan = build_for(
            &[("a", &["b", "c"]), ("b", &["d"]), ("c", &["d"]), ("d", &[])],
            "a",
        )
        .unwrap();
        assert_eq!(render(&plan, "a"), "a\n├─ b\n│  └─ d\n└─ c\n   └─ d\n");
    }

    #[test]
    fn unknown_command_is_reported() {
        let err = build_for(&[("a", &[])], "nope").unwrap_err();
        assert!(matches!(err, Error::UnknownCommand { name, .. } if name == "nope"));
    }

    #[test]
    fn unknown_dependency_is_reported() {
        let err = build_for(&[("a", &["ghost"])], "a").unwrap_err();
        assert!(matches!(err, Error::UnknownCommand { name, .. } if name == "ghost"));
    }

    #[test]
    fn dependency_cycle_is_detected() {
        let err = build_for(&[("a", &["b"]), ("b", &["a"])], "a").unwrap_err();
        assert!(matches!(err, Error::DependencyCycle(_)));
    }
}
