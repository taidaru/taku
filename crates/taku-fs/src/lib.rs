use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use mlua::Lua;

fn err<E: std::fmt::Display>(ctx: &str, e: E) -> mlua::Error {
    mlua::Error::external(format!("{ctx}: {e}"))
}

pub trait FileSystem: Send + Sync {
    fn read(&self, path: &str) -> mlua::Result<Vec<u8>>;
    fn write(&self, path: &str, contents: &[u8]) -> mlua::Result<()>;
    fn append(&self, path: &str, contents: &[u8]) -> mlua::Result<()>;
    fn exists(&self, path: &str) -> bool;
    fn is_file(&self, path: &str) -> bool;
    fn is_dir(&self, path: &str) -> bool;
    fn mkdir(&self, path: &str) -> mlua::Result<()>;
    fn remove(&self, path: &str) -> mlua::Result<()>;
    fn copy(&self, src: &str, dst: &str) -> mlua::Result<()>;
    fn rename(&self, src: &str, dst: &str) -> mlua::Result<()>;
    fn read_dir(&self, path: &str) -> mlua::Result<Vec<String>>;
}

pub struct Local;

impl FileSystem for Local {
    fn read(&self, path: &str) -> mlua::Result<Vec<u8>> {
        fs::read(path).map_err(|e| err(&format!("fs.read({path})"), e))
    }
    fn write(&self, path: &str, contents: &[u8]) -> mlua::Result<()> {
        fs::write(path, contents).map_err(|e| err(&format!("fs.write({path})"), e))
    }
    fn append(&self, path: &str, contents: &[u8]) -> mlua::Result<()> {
        (|| {
            let mut f = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            f.write_all(contents)
        })()
        .map_err(|e| err(&format!("fs.append({path})"), e))
    }
    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }
    fn is_file(&self, path: &str) -> bool {
        Path::new(path).is_file()
    }
    fn is_dir(&self, path: &str) -> bool {
        Path::new(path).is_dir()
    }
    fn mkdir(&self, path: &str) -> mlua::Result<()> {
        fs::create_dir_all(path).map_err(|e| err(&format!("fs.mkdir({path})"), e))
    }
    fn remove(&self, path: &str) -> mlua::Result<()> {
        let p = Path::new(path);
        let res = if p.is_dir() {
            fs::remove_dir_all(p)
        } else {
            fs::remove_file(p)
        };
        res.map_err(|e| err(&format!("fs.remove({path})"), e))
    }
    fn copy(&self, src: &str, dst: &str) -> mlua::Result<()> {
        fs::copy(src, dst)
            .map(|_| ())
            .map_err(|e| err(&format!("fs.copy({src} -> {dst})"), e))
    }
    fn rename(&self, src: &str, dst: &str) -> mlua::Result<()> {
        fs::rename(src, dst).map_err(|e| err(&format!("fs.rename({src} -> {dst})"), e))
    }
    fn read_dir(&self, path: &str) -> mlua::Result<Vec<String>> {
        let entries = fs::read_dir(path).map_err(|e| err(&format!("fs.read_dir({path})"), e))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| err(&format!("fs.read_dir({path})"), e))?;
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        Ok(names)
    }
}

pub fn register(lua: &Lua, fs: Arc<dyn FileSystem>) -> mlua::Result<()> {
    let tbl = lua.create_table()?;

    let f = fs.clone();
    tbl.set(
        "read",
        lua.create_function(move |lua, path: String| lua.create_string(f.read(&path)?))?,
    )?;

    let f = fs.clone();
    tbl.set(
        "write",
        lua.create_function(move |_, (path, contents): (String, mlua::String)| {
            f.write(&path, &contents.as_bytes())
        })?,
    )?;

    let f = fs.clone();
    tbl.set(
        "append",
        lua.create_function(move |_, (path, contents): (String, mlua::String)| {
            f.append(&path, &contents.as_bytes())
        })?,
    )?;

    let f = fs.clone();
    tbl.set(
        "exists",
        lua.create_function(move |_, path: String| Ok(f.exists(&path)))?,
    )?;

    let f = fs.clone();
    tbl.set(
        "is_file",
        lua.create_function(move |_, path: String| Ok(f.is_file(&path)))?,
    )?;

    let f = fs.clone();
    tbl.set(
        "is_dir",
        lua.create_function(move |_, path: String| Ok(f.is_dir(&path)))?,
    )?;

    let f = fs.clone();
    tbl.set(
        "mkdir",
        lua.create_function(move |_, path: String| f.mkdir(&path))?,
    )?;

    let f = fs.clone();
    tbl.set(
        "remove",
        lua.create_function(move |_, path: String| f.remove(&path))?,
    )?;

    let f = fs.clone();
    tbl.set(
        "copy",
        lua.create_function(move |_, (src, dst): (String, String)| f.copy(&src, &dst))?,
    )?;

    let f = fs.clone();
    tbl.set(
        "rename",
        lua.create_function(move |_, (src, dst): (String, String)| f.rename(&src, &dst))?,
    )?;

    tbl.set(
        "read_dir",
        lua.create_function(move |lua, path: String| {
            let list = lua.create_table()?;
            for (i, name) in fs.read_dir(&path)?.into_iter().enumerate() {
                list.set(i + 1, name)?;
            }
            Ok(list)
        })?,
    )?;

    lua.globals().set("fs", tbl)?;
    Ok(())
}
