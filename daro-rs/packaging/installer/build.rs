//! 构建脚本：定位已编译好的 daro-rs.exe，并把它的（绝对、正斜杠）路径
//! 写进一个 embed.rs，再通过 `cargo:rustc-env` 暴露给 main.rs 用 include! 引入。
//! 走 env 变量而不是把路径写死在 include_bytes! 字面量里，是为了规避 Windows
//! 反斜杠在 Rust 字符串字面量里被当成转义符的问题。

use std::fs;
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    // packaging/installer -> ../../target/release/daro-rs.exe
    let src = Path::new(&manifest)
        .join("../../target/release/daro-rs.exe")
        .canonicalize()
        .expect("未找到待打包的 daro-rs.exe，请先在 daro-rs 目录执行 cargo build --release");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let embed_rs = Path::new(&out_dir).join("embed.rs");

    // 关键：把路径里的反斜杠替换为正斜杠，确保生成的字符串字面量合法。
    let exe_fwd = src.to_string_lossy().replace('\\', "/");
    let code = format!("pub static DARO_EXE: &[u8] = include_bytes!(r\"{exe_fwd}\");\n");
    fs::write(&embed_rs, code).expect("写入 embed.rs 失败");

    let embed_rs_fwd = embed_rs.to_string_lossy().replace('\\', "/");
    println!("cargo:rustc-env=DARO_EMBED={embed_rs_fwd}");
    println!("cargo:rerun-if-changed={exe_fwd}");
}
