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

pub fn rm(path: &str) -> mlua::Result<()> {
    let p = Path::new(path);
    // symlink_metadata so a symlink to a directory is removed as the
    // link itself, never by descending into its target.
    let res = match fs::symlink_metadata(p) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(p),
        _ => fs::remove_file(p),
    };
    res.map_err(|e| ext(&format!("fs.rm({path})"), e))
}

pub fn cp(src: &str, dst: &str) -> mlua::Result<()> {
    fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| ext(&format!("fs.cp({src} -> {dst})"), e))
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

pub fn register(lua: &mlua::Lua) -> mlua::Result<()> {
    taku_api::lua_api!(lua, global = "fs" {
        read => |lua, path: String| lua.create_string(read(&path)?),
        write => |_, (path, contents): (String, mlua::String)| write(&path, &contents.as_bytes()),
        append => |_, (path, contents): (String, mlua::String)| append(&path, &contents.as_bytes()),
        exists => |_, path: String| Ok(Path::new(&path).exists()),
        is_file => |_, path: String| Ok(Path::new(&path).is_file()),
        is_dir => |_, path: String| Ok(Path::new(&path).is_dir()),
        mkdir => |_, path: String| mkdir(&path),
        rm => |_, path: String| rm(&path),
        cp => |_, (src, dst): (String, String)| cp(&src, &dst),
        mv => |_, (src, dst): (String, String)| mv(&src, &dst),
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
}
