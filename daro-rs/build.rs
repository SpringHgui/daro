use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/icons/ui/logo.ico");
    println!("cargo:rerun-if-changed=assets/icons/legacy");
    let _ = embed_resource::compile("assets/app.rc", embed_resource::NONE);
    emit_legacy_index();
}

/// 参考客户端图标按 16/24/32/48/64/128 分目录存放；这里把目录清单编成 Rust 数组，
/// 免得每加一张图都要手写一行 include_bytes!。128 档只给信息卡 64px 大图标用，
/// 素材由原生档 Lanczos 放大（见 SOURCES.json）。
fn emit_legacy_index() {
    let root = PathBuf::from("assets/icons/legacy");
    let mut code = String::from("pub static LEGACY_ICONS: &[(u16, &str, &[u8])] = &[\n");
    for class in [16u16, 24, 32, 48, 64, 128] {
        let dir = root.join(format!("size{class}"));
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok().map(|e| e.path()))
                    .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        for path in files {
            let id = path.file_stem().unwrap().to_string_lossy().into_owned();
            let abs = env::current_dir().unwrap().join(&path);
            code.push_str(&format!(
                "    ({class}, {id:?}, include_bytes!({path:?})),\n",
                path = abs.to_string_lossy()
            ));
        }
    }
    code.push_str("];\n");
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("legacy_index.rs");
    fs::write(out, code).unwrap();
}
