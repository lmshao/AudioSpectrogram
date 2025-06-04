use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("build_time.rs");
    let mut f = File::create(dest_path).unwrap();

    let build_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    write!(f, "pub const BUILD_TIME: &str = \"{}\";", build_time).unwrap();

    if cfg!(target_os = "windows") {
        let _ = embed_resource::compile("resources.rc", Vec::<&str>::new());
    }
}
