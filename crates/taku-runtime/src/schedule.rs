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

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute(
    style: &Style,
    path: &Path,
    source: &str,
    plan: &Plan,
    apis: &'static [taku_api::ApiEntry],
    target: &str,
    opts: &crate::RunOpts,
    overrides: &HashMap<String, String>,
) -> Result<usize, Error> {
    let done = crate::exec::Done::default();
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

    // a dry run is sequential so its printed plan doesn't interleave
    let max = if opts.dry_run {
        1
    } else {
        opts.jobs
            .or_else(|| std::thread::available_parallelism().ok())
            .map_or(1, NonZeroUsize::get)
    };
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
                // --vars targets only the task the user asked for, not deps
                let overrides = (name == target).then_some(overrides);
                let name = name.to_string();
                let done = done.clone();
                scope.spawn(move || {
                    let start = Instant::now();
                    let res = run_task(
                        path,
                        source,
                        &name,
                        &worker_style,
                        apis,
                        overrides,
                        opts,
                        &done,
                    );
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

#[allow(clippy::too_many_arguments)]
fn run_task(
    path: &Path,
    source: &str,
    name: &str,
    style: &Style,
    apis: &'static [taku_api::ApiEntry],
    overrides: Option<&HashMap<String, String>>,
    opts: &crate::RunOpts,
    done: &crate::exec::Done,
) -> Result<(), String> {
    let body = AssertUnwindSafe(|| run_task_body(path, source, name, apis, overrides, opts, done));
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
    overrides: Option<&HashMap<String, String>>,
    opts: &crate::RunOpts,
    done: &crate::exec::Done,
) -> Result<(), Error> {
    // completed by any path already (e.g. `invoke`d by another task) — skip
    if done.lock().unwrap().contains(name) {
        return Ok(());
    }
    let (lua, dotenv) = build_state(path, source, false, apis)?;
    let tasks: Table = lua.named_registry_value(TASKS_KEY)?;
    let spec: Table = tasks
        .get::<Option<Table>>(name)?
        .ok_or_else(|| Error::UnknownCommand {
            name: name.to_string(),
            takufile: path.to_path_buf(),
            available: Vec::new(),
        })?;
    // placeholder priority: param defaults < --vars < ctx.vars set by steps
    let mut vars = crate::exec::initial_vars(&spec)?;
    if let Some(o) = overrides {
        vars.extend(o.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    let mut ctx = crate::exec::Ctx {
        dotenv,
        vars,
        base: path.parent().map_or_else(|| ".".into(), Path::to_path_buf),
        yes: opts.yes,
        force: opts.force,
        explain: opts.explain,
        dry_run: opts.dry_run,
        done: done.clone(),
        pending_state: None,
    };
    if opts.dry_run {
        println!("{name}:");
    }
    // Effects (`cmd.*`, fs writes, `net.*`) are gated on the runtime phase;
    // only step execution — never top-level Takufile code — may perform them.
    taku_api::set_runtime(true);
    let result = crate::exec::run_steps(&lua, apis, &spec, &mut ctx);
    taku_api::set_runtime(false);
    result?;
    done.lock().unwrap().insert(name.to_string());
    Ok(())
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
        run_task_body(
            &path,
            &source,
            "t",
            crate::test_apis(),
            None,
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
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

        run_task_body(
            &path,
            &source,
            "files",
            crate::test_apis(),
            None,
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        let b = std::fs::read_to_string(dir.join("out/b.txt")).unwrap();
        assert_eq!(b, "hello greetingmore\n");
        assert!(dir.join("out/via-command").is_file());
        assert!(!dir.join("out/a.txt").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn function_steps_get_ctx_and_vars_feed_later_steps() {
        let dir = std::env::temp_dir().join(format!("taku-ctx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Takufile.lua");
        let d = dir.display();

        let source = format!(
            r#"
task("t <sha=none>", {{
    function(ctx)
        assert(ctx.task.name == "t", "ctx.task.name")
        assert(ctx.vars.sha == "none", "param default visible")
        ctx.vars.sha = "abc"
        fs.write("{d}/fmt.txt", fmt("v=${{sha}}"))
    end,
    "touch {d}/f-${{sha}}",
}})

task("o <x=a>", {{ "touch {d}/o-${{x}}" }})
"#
        );
        run_task_body(
            &path,
            &source,
            "t",
            crate::test_apis(),
            None,
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("fmt.txt")).unwrap(),
            "v=abc"
        );
        assert!(dir.join("f-abc").is_file());

        let overrides = HashMap::from([("x".to_string(), "b".to_string())]);
        run_task_body(
            &path,
            &source,
            "o",
            crate::test_apis(),
            Some(&overrides),
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        assert!(dir.join("o-b").is_file());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_invoked_task_satisfies_a_later_dep_and_yes_answers_confirm() {
        let dir = std::env::temp_dir().join(format!("taku-done-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Takufile.lua");
        let d = dir.display();

        let source = format!(
            r#"
task("db-reset", {{
    confirm "wipe the db?",
    append {{ "ran", to = "{d}/log.txt" }},
}})

task("t1", {{ invoke "db-reset" }})
"#
        );
        let done = crate::exec::Done::default();
        let yes_opts = crate::RunOpts {
            yes: true,
            ..Default::default()
        };
        run_task_body(
            &path,
            &source,
            "t1",
            crate::test_apis(),
            None,
            &yes_opts,
            &done,
        )
        .unwrap();
        assert!(done.lock().unwrap().contains("db-reset"));

        // the scheduler would now run db-reset as a dep — it must be a no-op
        run_task_body(
            &path,
            &source,
            "db-reset",
            crate::test_apis(),
            None,
            &yes_opts,
            &done,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("log.txt")).unwrap(),
            "ran\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unchanged_guard_skips_reruns_on_input_change_and_obeys_force() {
        let dir = std::env::temp_dir().join(format!("taku-inc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Takufile.lua");
        let d = dir.display();
        std::fs::write(dir.join("input.txt"), "one").unwrap();

        let source = format!(
            r#"
task("build", {{
    append {{ "pre", to = "{d}/pre.log" }},
    unchanged {{ "input.txt", outputs = "out.txt" }},
    append {{ "ran", to = "{d}/run.log" }},
    write {{ "artifact", to = "{d}/out.txt" }},
}})
"#
        );
        let lines =
            |file: &str| std::fs::read_to_string(dir.join(file)).map_or(0, |s| s.lines().count());
        let run = |opts: &crate::RunOpts| {
            run_task_body(
                &path,
                &source,
                "build",
                crate::test_apis(),
                None,
                opts,
                &Default::default(),
            )
            .unwrap();
        };

        run(&Default::default()); // cold: everything runs, state recorded
        assert_eq!((lines("pre.log"), lines("run.log")), (1, 1));

        run(&Default::default()); // warm: pre-guard step runs, the rest skips
        assert_eq!((lines("pre.log"), lines("run.log")), (2, 1));

        std::fs::remove_file(dir.join("out.txt")).unwrap();
        run(&Default::default()); // outputs missing: rebuild
        assert_eq!(lines("run.log"), 2);

        std::fs::write(dir.join("input.txt"), "two").unwrap();
        run(&Default::default()); // input changed: rebuild
        assert_eq!(lines("run.log"), 3);

        run(&crate::RunOpts {
            force: true,
            ..Default::default()
        });
        assert_eq!(lines("run.log"), 4);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dry_run_touches_nothing() {
        let dir = std::env::temp_dir().join(format!("taku-dry-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Takufile.lua");
        let d = dir.display();

        let source = format!(
            r#"
task("t", {{
    mkdir "{d}/made",
    function(ctx) fs.write("{d}/from-fn.txt", "x") end,
    "touch {d}/from-cmd",
}})
"#
        );
        run_task_body(
            &path,
            &source,
            "t",
            crate::test_apis(),
            None,
            &crate::RunOpts {
                dry_run: true,
                ..Default::default()
            },
            &Default::default(),
        )
        .unwrap();
        assert!(!dir.join("made").exists());
        assert!(!dir.join("from-fn.txt").exists());
        assert!(!dir.join("from-cmd").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
