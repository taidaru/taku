use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use mlua::Table;

use crate::error::Error;
use crate::plan::Plan;
use crate::report::{self, Style};
use crate::state::{TASKS_KEY, build_state};

pub(crate) fn execute(
    style: &Style,
    path: &Path,
    source: &str,
    plan: &Plan,
    jobs: Option<NonZeroUsize>,
    apis: &'static [taku_api::ApiEntry],
) -> Result<usize, Error> {
    let mut indegree: HashMap<&str, usize> = plan
        .deps
        .iter()
        .map(|(k, v)| (k.as_str(), v.len()))
        .collect();

    // reverse edges: task -> tasks that depend on it
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for (task, ds) in &plan.deps {
        for d in ds {
            dependents
                .entry(d.as_str())
                .or_default()
                .push(task.as_str());
        }
    }

    let mut roots: Vec<&str> = indegree
        .iter()
        .filter(|&(_, &d)| d == 0)
        .map(|(k, _)| *k)
        .collect();
    roots.sort();
    let mut ready: VecDeque<&str> = roots.into();

    let max = jobs
        .or_else(|| std::thread::available_parallelism().ok())
        .map_or(1, NonZeroUsize::get);
    let mut running = 0;
    let mut first_err: Option<Error> = None;
    let worker_style = *style;

    std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel::<(String, Result<(), String>, Duration)>();
        loop {
            // launch ready tasks up to the job limit
            while first_err.is_none() && running < max {
                let Some(name) = ready.pop_front() else { break };
                let tx = tx.clone();
                let name = name.to_string();
                scope.spawn(move || {
                    let start = Instant::now();
                    let res = run_task(path, source, &name, &worker_style, apis);
                    let _ = tx.send((name, res, start.elapsed()));
                });
                running += 1;
            }

            if running == 0 {
                break;
            }

            // wait for one task to finish, then unblock its dependents
            let Ok((name, res, elapsed)) = rx.recv() else {
                break;
            };
            running -= 1;
            match res {
                Ok(()) => {
                    report::task_done(style, &name, elapsed);
                    if let Some(ds) = dependents.get(name.as_str()) {
                        for d in ds {
                            if let Some(count) = indegree.get_mut(*d) {
                                *count -= 1;
                                if *count == 0 && first_err.is_none() {
                                    ready.push_back(*d);
                                }
                            }
                        }
                    }
                }
                Err(message) => {
                    report::task_failed(style, &name, elapsed);
                    if first_err.is_none() {
                        first_err = Some(Error::TaskFailed(message));
                    } else {
                        eprintln!("{}", style.error(&message));
                    }
                }
            }
        }
    });

    match first_err {
        Some(e) => Err(e),
        None => Ok(plan.tasks.len()),
    }
}

fn run_task(
    path: &Path,
    source: &str,
    name: &str,
    style: &Style,
    apis: &'static [taku_api::ApiEntry],
) -> Result<(), String> {
    let body = AssertUnwindSafe(|| run_task_body(path, source, name, apis));
    match catch_unwind(body) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(Error::Lua(e))) => Err(format!(
            "task '{name}' failed\n{}",
            crate::diagnostic::render(&e, style)
        )),
        Ok(Err(e)) => Err(format!("task '{name}' failed: {e}")),
        Err(payload) => Err(format!(
            "task '{name}' panicked: {}",
            panic_message(payload.as_ref())
        )),
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

fn run_task_body(
    path: &Path,
    source: &str,
    name: &str,
    apis: &'static [taku_api::ApiEntry],
) -> Result<(), Error> {
    let (lua, dotenv) = build_state(path, source, false, apis)?;
    let tasks: Table = lua.named_registry_value(TASKS_KEY)?;
    let spec: Table = tasks
        .get::<Option<Table>>(name)?
        .ok_or_else(|| Error::UnknownCommand {
            name: name.to_string(),
            takufile: path.to_path_buf(),
            available: Vec::new(),
        })?;
    let mut ctx = crate::exec::Ctx {
        dotenv,
        vars: crate::exec::initial_vars(&spec)?,
    };
    // Effects (`cmd.*`, fs writes, `net.*`) are gated on the runtime phase;
    // only step execution — never top-level Takufile code — may perform them.
    taku_api::set_runtime(true);
    let result = crate::exec::run_steps(&lua, apis, &spec, &mut ctx);
    taku_api::set_runtime(false);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_fail_at_load_but_work_in_a_task_body() {
        let dir = std::env::temp_dir().join(format!("taku-phase-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Takufile.lua");
        let sub = dir.join("made-by-task").display().to_string();

        let top_level = format!("fs.mkdir('{sub}')");
        let err = build_state(&path, &top_level, false, crate::test_apis()).unwrap_err();
        assert!(
            err.to_string()
                .contains("only available while a task is running"),
            "got: {err}"
        );

        let source = format!("task('t', {{ function() fs.mkdir('{sub}') end }})");
        run_task_body(&path, &source, "t", crate::test_apis()).unwrap();
        assert!(std::path::Path::new(&sub).is_dir());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn data_steps_execute_and_deps_come_from_the_header() {
        let dir = std::env::temp_dir().join(format!("taku-steps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Takufile.lua");
        let d = dir.display();

        let source = format!(
            r#"
--- writes and rearranges files
task("files <name=greeting>", {{
    mkdir "{d}/out",
    write {{ "hello ${{name}}", to = "{d}/out/a.txt" }},
    cp {{ "{d}/out/a.txt", to = "{d}/out/b.txt" }},
    append {{ "more", to = "{d}/out/b.txt" }},
    "touch {d}/out/via-command",
    rm "{d}/out/a.txt",
}})

task("all: files", {{}})
"#
        );
        let (lua, _env) = build_state(&path, &source, false, crate::test_apis()).unwrap();
        let plan = crate::plan::build(&lua, &path, "all").unwrap();
        assert_eq!(plan.deps["all"], ["files"]);

        run_task_body(&path, &source, "files", crate::test_apis()).unwrap();
        let b = std::fs::read_to_string(dir.join("out/b.txt")).unwrap();
        assert_eq!(b, "hello greetingmore\n");
        assert!(dir.join("out/via-command").is_file());
        assert!(!dir.join("out/a.txt").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
