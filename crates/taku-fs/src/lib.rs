use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

fn err<E: std::fmt::Display>(ctx: &str, e: E) -> mlua::Error {
    mlua::Error::external(format!("{ctx}: {e}"))
}

pub trait FileSystem: Send + Sync {
    fn read(&self, path: &str) -> mlua::Result<Vec<u8>>;
    fn write(&self, path: &str, contents: &[u8]) -> mlua::Result<()>;
    fn append(&self, path: &str, contents: &[u8]) -> mlua::Result<()>;
    fn exists(&self, path: &str) -> mlua::Result<bool>;
    fn is_file(&self, path: &str) -> mlua::Result<bool>;
    fn is_dir(&self, path: &str) -> mlua::Result<bool>;
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
    fn exists(&self, path: &str) -> mlua::Result<bool> {
        Ok(Path::new(path).exists())
    }
    fn is_file(&self, path: &str) -> mlua::Result<bool> {
        Ok(Path::new(path).is_file())
    }
    fn is_dir(&self, path: &str) -> mlua::Result<bool> {
        Ok(Path::new(path).is_dir())
    }
    fn mkdir(&self, path: &str) -> mlua::Result<()> {
        fs::create_dir_all(path).map_err(|e| err(&format!("fs.mkdir({path})"), e))
    }
    fn remove(&self, path: &str) -> mlua::Result<()> {
        let p = Path::new(path);
        // symlink_metadata so a symlink to a directory is removed as the
        // link itself, never by descending into its target.
        let res = match fs::symlink_metadata(p) {
            Ok(meta) if meta.is_dir() => fs::remove_dir_all(p),
            _ => fs::remove_file(p),
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
        // OS iteration order is arbitrary; sort for deterministic tasks.
        names.sort();
        Ok(names)
    }
}

pub const API: taku_api::ApiEntry = taku_api::ApiEntry {
    global: "fs",
    register: |lua, _ctx| register(lua, Arc::new(Local)),
};

taku_api::lua_api! {
    pub fn register(global = "fs", backend: FileSystem as f) {
        read => |lua, path: String| lua.create_string(f.read(&path)?),
        write => |_, (path, contents): (String, mlua::String)| {
            f.write(&path, &contents.as_bytes())
        },
        append => |_, (path, contents): (String, mlua::String)| {
            f.append(&path, &contents.as_bytes())
        },
        exists => |_, path: String| f.exists(&path),
        is_file => |_, path: String| f.is_file(&path),
        is_dir => |_, path: String| f.is_dir(&path),
        mkdir => |_, path: String| f.mkdir(&path),
        remove => |_, path: String| f.remove(&path),
        copy => |_, (src, dst): (String, String)| f.copy(&src, &dst),
        rename => |_, (src, dst): (String, String)| f.rename(&src, &dst),
        read_dir => |lua, path: String| {
            let list = lua.create_table()?;
            for (i, name) in f.read_dir(&path)?.into_iter().enumerate() {
                list.set(i + 1, name)?;
            }
            Ok(list)
        },
    }
}
