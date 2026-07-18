use std::fs;
use std::io::Write;
use std::path::Path;

use taku_api::ext;

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

pub fn remove(path: &str) -> mlua::Result<()> {
    let p = Path::new(path);
    // symlink_metadata so a symlink to a directory is removed as the
    // link itself, never by descending into its target.
    let res = match fs::symlink_metadata(p) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(p),
        _ => fs::remove_file(p),
    };
    res.map_err(|e| ext(&format!("fs.remove({path})"), e))
}

pub fn copy(src: &str, dst: &str) -> mlua::Result<()> {
    fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| ext(&format!("fs.copy({src} -> {dst})"), e))
}

pub fn rename(src: &str, dst: &str) -> mlua::Result<()> {
    fs::rename(src, dst).map_err(|e| ext(&format!("fs.rename({src} -> {dst})"), e))
}

pub fn read_dir(path: &str) -> mlua::Result<Vec<String>> {
    let entries = fs::read_dir(path).map_err(|e| ext(&format!("fs.read_dir({path})"), e))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| ext(&format!("fs.read_dir({path})"), e))?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    // OS iteration order is arbitrary; sort for deterministic tasks.
    names.sort();
    Ok(names)
}

pub fn register(lua: &mlua::Lua) -> mlua::Result<()> {
    taku_api::lua_api!(lua, global = "fs" {
        read => |lua, path: String| lua.create_string(read(&path)?),
        write => |_, (path, contents): (String, mlua::String)| write(&path, &contents.as_bytes()),
        append => |_, (path, contents): (String, mlua::String)| append(&path, &contents.as_bytes()),
        exists => |_, path: String| Ok(Path::new(&path).exists()),
        is_file => |_, path: String| Ok(Path::new(&path).is_file()),
        is_dir => |_, path: String| Ok(Path::new(&path).is_dir()),
        mkdir => |_, path: String| mkdir(&path),
        remove => |_, path: String| remove(&path),
        copy => |_, (src, dst): (String, String)| copy(&src, &dst),
        rename => |_, (src, dst): (String, String)| rename(&src, &dst),
        read_dir => |lua, path: String| {
            let list = lua.create_table()?;
            for (i, name) in read_dir(&path)?.into_iter().enumerate() {
                list.set(i + 1, name)?;
            }
            Ok(list)
        },
    })
}
