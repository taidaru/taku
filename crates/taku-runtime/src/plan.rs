use std::collections::HashMap;
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
        dep_names.push(dep);
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
