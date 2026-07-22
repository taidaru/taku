use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use mlua::{Lua, Table};
use taku_api::ext;
use taku_api::steps::{Arg, Field, FieldKind, Positional, StepCtx, StepDef};

/// `{ <payload>|[1], to = "..." }` — the shared shape of `cp`/`mv`/`write`/
/// `append`: a required payload (positional or its named field) plus a
/// required `to`. The payload field is not itself `required` — the positional
/// check enforces it, satisfied by `[1]` or the field.
const fn to_fields(payload: &'static str) -> [Field; 2] {
    [
        Field {
            name: payload,
            kind: FieldKind::Str,
            required: false,
        },
        Field {
            name: "to",
            kind: FieldKind::Str,
            required: true,
        },
    ]
}
const PATH_TO: &[Field] = &to_fields("src");
const DATA_TO: &[Field] = &to_fields("data");
const LINE_TO: &[Field] = &to_fields("line");

const fn payload(what: &'static str, field: &'static str) -> Positional {
    Positional {
        what,
        suggest: field,
        help: "add it as the first element or a named field",
        field: Some(field),
    }
}

pub fn read(path: &str) -> mlua::Result<Vec<u8>> {
    fs::read(path).map_err(|e| ext(&format!("fs.read({path})"), e))
}

pub fn write(path: &str, contents: &[u8]) -> mlua::Result<()> {
    fs::write(path, contents).map_err(|e| ext(&format!("fs.write({path})"), e))
}

pub fn append(path: &str, contents: &[u8]) -> mlua::Result<()> {
    (|| {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        f.write_all(contents)
    })()
    .map_err(|e| ext(&format!("fs.append({path})"), e))
}

pub fn mkdir(path: &str) -> mlua::Result<()> {
    fs::create_dir_all(path).map_err(|e| ext(&format!("fs.mkdir({path})"), e))
}

pub fn rm(path: &str) -> mlua::Result<()> {
    let p = Path::new(path);
    // symlink_metadata so a symlink to a directory is removed as the
    // link itself, never by descending into its target.
    let res = match fs::symlink_metadata(p) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(p),
        Ok(_) => fs::remove_file(p),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => Err(e),
    };
    res.map_err(|e| ext(&format!("fs.rm({path})"), e))
}

pub fn cp(src: &str, dst: &str) -> mlua::Result<()> {
    let mut seen = HashSet::new();
    copy_recursive(Path::new(src), Path::new(dst), &mut seen)
        .map_err(|e| ext(&format!("fs.cp({src} -> {dst})"), e))
}

/// Copies a file, or a whole directory tree, following symlinks — `fs::copy`
/// alone only handles single files. `seen` tracks canonical directory paths so
/// a symlink cycle (e.g. `src/link -> src/`) stops instead of recursing to ELOOP.
fn copy_recursive(src: &Path, dst: &Path, seen: &mut HashSet<PathBuf>) -> io::Result<()> {
    if fs::metadata(src)?.is_dir() {
        if let Ok(real) = fs::canonicalize(src)
            && !seen.insert(real)
        {
            return Ok(());
        }
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()), seen)?;
        }
        Ok(())
    } else {
        fs::copy(src, dst).map(|_| ())
    }
}

pub fn mv(src: &str, dst: &str) -> mlua::Result<()> {
    fs::rename(src, dst).map_err(|e| ext(&format!("fs.mv({src} -> {dst})"), e))
}

pub fn ls(path: &str) -> mlua::Result<Vec<String>> {
    let entries = fs::read_dir(path).map_err(|e| ext(&format!("fs.ls({path})"), e))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| ext(&format!("fs.ls({path})"), e))?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    // OS iteration order is arbitrary; sort for deterministic tasks.
    names.sort();
    Ok(names)
}

pub fn glob(pattern: &str) -> mlua::Result<Vec<String>> {
    let paths = glob::glob(pattern).map_err(|e| ext(&format!("fs.glob({pattern})"), e))?;
    let mut out = Vec::new();
    for path in paths {
        let path = path.map_err(|e| ext(&format!("fs.glob({pattern})"), e))?;
        out.push(path.to_string_lossy().into_owned());
    }
    // glob yields in filesystem order; sort for deterministic tasks.
    out.sort();
    Ok(out)
}

/// `{ src|[1], to = ... }` with both sides formatted.
fn src_to(t: &Table, ctx: &StepCtx) -> mlua::Result<(String, String)> {
    let src = ctx.fmt_field_or_first(t, "src")?;
    let to = ctx.fmt_value(t.get("to")?)?;
    Ok((src, to))
}

pub const API: taku_api::ApiEntry = taku_api::ApiEntry {
    globals: &["fs"],
    register: |lua, _ctx| register(lua),
    steps: &[
        StepDef::simple("rm", Arg::Str, |_, t, ctx| rm(&ctx.fmt_value(t.get(1)?)?)),
        StepDef::simple("mkdir", Arg::Str, |_, t, ctx| {
            mkdir(&ctx.fmt_value(t.get(1)?)?)
        }),
        StepDef {
            tag: "cp",
            arg: Arg::Table,
            run: |_, t, ctx| {
                let (src, to) = src_to(t, ctx)?;
                cp(&src, &to)
            },
            fields: PATH_TO,
            positional: Some(payload("source path", "src")),
        },
        StepDef {
            tag: "mv",
            arg: Arg::Table,
            run: |_, t, ctx| {
                let (src, to) = src_to(t, ctx)?;
                mv(&src, &to)
            },
            fields: PATH_TO,
            positional: Some(payload("source path", "src")),
        },
        StepDef {
            tag: "write",
            arg: Arg::Table,
            run: |_, t, ctx| {
                let data = ctx.fmt_field_or_first(t, "data")?;
                let to = ctx.fmt_value(t.get("to")?)?;
                write(&to, data.as_bytes())
            },
            fields: DATA_TO,
            positional: Some(payload("content", "data")),
        },
        StepDef {
            tag: "append",
            arg: Arg::Table,
            run: |_, t, ctx| {
                let line = ctx.fmt_field_or_first(t, "line")?;
                let to = ctx.fmt_value(t.get("to")?)?;
                append(&to, format!("{line}\n").as_bytes())
            },
            fields: LINE_TO,
            positional: Some(payload("line", "line")),
        },
    ],
};

pub fn register(lua: &Lua) -> mlua::Result<()> {
    taku_api::lua_api!(lua, global = "fs" {
        read => |lua, path: String| lua.create_string(read(&path)?),
        write => |_, (path, contents): (String, mlua::String)| {
            taku_api::require_runtime("fs.write")?;
            write(&path, &contents.as_bytes())
        },
        append => |_, (path, contents): (String, mlua::String)| {
            taku_api::require_runtime("fs.append")?;
            append(&path, &contents.as_bytes())
        },
        exists => |_, path: String| Ok(Path::new(&path).exists()),
        is_file => |_, path: String| Ok(Path::new(&path).is_file()),
        is_dir => |_, path: String| Ok(Path::new(&path).is_dir()),
        mkdir => |_, path: String| {
            taku_api::require_runtime("fs.mkdir")?;
            mkdir(&path)
        },
        rm => |_, path: String| {
            taku_api::require_runtime("fs.rm")?;
            rm(&path)
        },
        cp => |_, (src, dst): (String, String)| {
            taku_api::require_runtime("fs.cp")?;
            cp(&src, &dst)
        },
        mv => |_, (src, dst): (String, String)| {
            taku_api::require_runtime("fs.mv")?;
            mv(&src, &dst)
        },
        ls => |lua, path: String| {
            let list = lua.create_table()?;
            for (i, name) in ls(&path)?.into_iter().enumerate() {
                list.set(i + 1, name)?;
            }
            Ok(list)
        },
        glob => |lua, pattern: String| {
            let list = lua.create_table()?;
            for (i, path) in glob(&pattern)?.into_iter().enumerate() {
                list.set(i + 1, path)?;
            }
            Ok(list)
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_matches_and_sorts() {
        let dir = std::env::temp_dir().join(format!("taku-fs-glob-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("b.txt"), "").unwrap();
        fs::write(dir.join("a.txt"), "").unwrap();
        fs::write(dir.join("c.log"), "").unwrap();

        let got = glob(&format!("{}/*.txt", dir.display())).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            got,
            vec![
                dir.join("a.txt").to_string_lossy().into_owned(),
                dir.join("b.txt").to_string_lossy().into_owned(),
            ]
        );
    }

    #[test]
    fn bad_glob_pattern_is_an_error() {
        assert!(glob("[").is_err());
    }

    #[test]
    fn cp_copies_a_directory_tree() {
        let root = std::env::temp_dir().join(format!("taku-fs-cp-{}", std::process::id()));
        let src = root.join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), "aaa").unwrap();
        fs::write(src.join("sub/b.txt"), "bbb").unwrap();

        let dst = root.join("dst");
        cp(&src.to_string_lossy(), &dst.to_string_lossy()).unwrap();

        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "aaa");
        assert_eq!(fs::read_to_string(dst.join("sub/b.txt")).unwrap(), "bbb");
        fs::remove_dir_all(&root).unwrap();
    }
}
