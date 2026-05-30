use std::{
    env,
    path::PathBuf,
};

mod utils {
    use std::{
        env,
        fs,
        io::{
            self,
        },
        path::Path,
    };

    #[rustfmt::skip]
    pub fn get_config_dir_name() -> &'static str {
        let os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
        let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("CARGO_CFG_TARGET_ARCH not set");

        match (os.as_str(), arch.as_str()) {
            ("windows", "aarch64") => "build_out_windows_arm64",
            ("windows", "x86_64")  => "build_out_windows_x86_64",
            ("windows", "x86")     => "build_out_windows_x86",

            ("android", "aarch64") => "build_out_android_arm64-v8a",
            ("android", "arm")     => "build_out_android_armeabi-v7a",
            ("android", "x86")     => "build_out_android_x86",
            ("android", "x86_64")  => "build_out_android_x86_64",

            ("ios", "aarch64")     => "build_out_ios_arm64",

            ("macos", "aarch64")   => "build_out_macos_arm64",
            ("macos", "x86_64")    => "build_out_macos_x86_64",

            ("linux", "aarch64")   => "build_out_linux_arm64",
            ("linux", "x86_64")    => "build_out_linux_x86_64",

            _ => panic!("Unsupported or missing config for target OS: {os}, Arch: {arch}"),
        }
    }

    pub fn extract_zip(zip_path: &Path, dest: &Path) -> io::Result<()> {
        let file = fs::File::open(zip_path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

            let out_path = match entry.enclosed_name() {
                Some(path) => dest.join(path),
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("ZIP 条目包含非法路径: {}", entry.name()),
                    ));
                }
            };

            if entry.is_dir() {
                fs::create_dir_all(&out_path)?;
            } else {
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut out_file = fs::File::create(&out_path)?;
                io::copy(&mut entry, &mut out_file)?;
            }
        }
        Ok(())
    }

    pub fn emit_link_libs() {
        let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
        if target_os == "android" {
            println!("cargo:rustc-link-lib=m");
        } else if target_os == "linux" || target_os == "macos" || target_os == "ios" {
            println!("cargo:rustc-link-lib=m");
            println!("cargo:rustc-link-lib=pthread");
        } else if target_os == "windows" {
            println!("cargo:rustc-link-lib=bcrypt");
        }
    }

    pub fn create_base_bindings() -> bindgen::Builder {
        bindgen::Builder::default()
            .header("wrapper.h")
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .allowlist_function("av_.*")
            .allowlist_function("avformat_.*")
            .allowlist_function("avcodec_.*")
            .allowlist_function("avio_.*")
            .allowlist_function("swr_.*")
            .allowlist_type("AV.*")
            .allowlist_type("Swr.*")
            .allowlist_var("AV_.*")
            .allowlist_var("AVERROR_.*")
            .allowlist_var("AVFMT_.*")
            .allowlist_var("AVSEEK.*")
    }
}

mod bundled {
    use std::{
        collections::BTreeSet,
        env,
        fs,
        path::Path,
    };

    use crate::utils;

    fn parse_log_line(
        line: &str,
        ffmpeg_dir: &Path,
        defines: &mut BTreeSet<(String, Option<String>)>,
        includes: &mut BTreeSet<String>,
        c_files: &mut BTreeSet<String>,
    ) {
        if (line.contains("-c -o ") || line.contains("-c -Fo")) && line.contains(".c") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for &part in &parts {
                if part.starts_with("-D") && !part.starts_with("-DBUILDING_") {
                    let define = &part[2..];
                    if let Some((k, v)) = define.split_once('=') {
                        defines.insert((k.to_string(), Some(v.to_string())));
                    } else {
                        defines.insert((define.to_string(), None));
                    }
                } else if let Some(inc) = part.strip_prefix("-I") {
                    if inc == "." {
                        continue;
                    }
                    for sep in ["ffmpeg/", "ffmpeg\\"] {
                        if let Some(idx) = inc.find(sep) {
                            let rel = &inc[idx + sep.len()..];
                            if !rel.is_empty() {
                                includes
                                    .insert(ffmpeg_dir.join(rel).to_string_lossy().into_owned());
                            }
                            break;
                        }
                    }
                } else if Path::new(part)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("c"))
                    && let Some(idx) = part.find("libav").or_else(|| part.find("libsw"))
                {
                    c_files.insert(part[idx..].replace('\\', "/"));
                }
            }
        }
    }

    pub fn build(manifest_dir: &Path, out_dir: &Path) {
        // 优先使用 vendor/ 下已解压的目录，方便直接修改 C 源码进行调试
        let vendor_ffmpeg = manifest_dir.join("vendor").join("ffmpeg_slim");
        let vendor_configs = manifest_dir.join("vendor").join("configs");

        let (ffmpeg_dir, configs_base) = if vendor_ffmpeg.exists() && vendor_configs.exists() {
            println!("cargo:rerun-if-changed=vendor/ffmpeg_slim");
            println!("cargo:rerun-if-changed=vendor/configs");
            (vendor_ffmpeg, vendor_configs)
        } else {
            let slim_zip = manifest_dir.join("vendor").join("ffmpeg_slim.zip");
            let configs_zip = manifest_dir.join("vendor").join("configs.zip");

            println!("cargo:rerun-if-changed=vendor/ffmpeg_slim.zip");
            println!("cargo:rerun-if-changed=vendor/configs.zip");

            let ffmpeg_dir = out_dir.join("ffmpeg_slim");
            let configs_base = out_dir.join("configs");

            if !ffmpeg_dir.exists() {
                utils::extract_zip(&slim_zip, &ffmpeg_dir)
                    .unwrap_or_else(|e| panic!("解压 ffmpeg_slim.zip 失败: {e}"));
            }
            if !configs_base.exists() {
                utils::extract_zip(&configs_zip, &configs_base)
                    .unwrap_or_else(|e| panic!("解压 configs.zip 失败: {e}"));
            }
            (ffmpeg_dir, configs_base)
        };

        let config_dir_name = utils::get_config_dir_name();
        let config_dir = configs_base.join(config_dir_name);
        let log_path = config_dir.join("make_dryrun.log");

        println!("cargo:rerun-if-changed={}", log_path.display());

        let log_content = fs::read_to_string(&log_path).unwrap_or_else(|_| {
            panic!(
                "无法读取日志文件: {}.\n请确认该目标平台的配置已包含在 configs.zip 中。",
                log_path.display()
            )
        });

        let mut c_files: BTreeSet<String> = BTreeSet::new();
        let mut defines: BTreeSet<(String, Option<String>)> = BTreeSet::new();
        let mut includes: BTreeSet<String> = BTreeSet::new();

        for line in log_content.lines() {
            parse_log_line(line, &ffmpeg_dir, &mut defines, &mut includes, &mut c_files);
        }

        let mut build = cc::Build::new();
        build.include(&ffmpeg_dir);
        build.include(&config_dir);
        build.include(ffmpeg_dir.join("libavcodec"));
        build.include(ffmpeg_dir.join("libavformat"));
        build.include(ffmpeg_dir.join("libswresample"));

        for inc in &includes {
            build.include(inc);
        }
        for (k, v) in &defines {
            build.define(k, v.as_deref());
        }
        for file in &c_files {
            build.file(ffmpeg_dir.join(file));
        }

        if env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
            build.flag("/utf-8");
        }
        build.warnings(false);
        build.compile("ffmpeg_audio");

        utils::emit_link_libs();

        println!("cargo:rerun-if-changed=wrapper.h");

        let mut builder = utils::create_base_bindings()
            .clang_arg(format!("-I{}", ffmpeg_dir.display()))
            .clang_arg(format!("-I{}", config_dir.display()));

        for inc in &includes {
            builder = builder.clang_arg(format!("-I{inc}"));
        }
        for (k, v) in defines {
            builder =
                builder.clang_arg(v.map_or_else(|| format!("-D{k}"), |val| format!("-D{k}={val}")));
        }

        let bindings = builder.generate().expect("无法生成 FFmpeg 绑定");
        bindings
            .write_to_file(out_dir.join("bindings.rs"))
            .expect("无法写入 bindings.rs");
    }
}

mod system {
    use std::path::{
        Path,
        PathBuf,
    };

    use crate::utils;

    const REQUIRED_LIBS: [&str; 4] = ["libavcodec", "libavformat", "libavutil", "libswresample"];

    pub fn build(out_dir: &Path, target_os: &str) {
        println!("cargo:warning=未找到 ffmpeg_slim.zip，正在尝试寻找已安装的 FFmpeg");
        println!("cargo:rerun-if-changed=wrapper.h");

        let mut include_paths: Vec<PathBuf> = Vec::new();

        if target_os == "windows" {
            let vcpkg_libs = ["avcodec", "avformat", "avutil", "swresample"];
            let mut all_found = true;
            for lib in &vcpkg_libs {
                if let Ok(library) = vcpkg::Config::new().emit_includes(true).probe(lib) {
                    include_paths.extend(library.include_paths);
                } else {
                    all_found = false;
                    break;
                }
            }
            if !all_found {
                include_paths.clear();
            }
        }

        if include_paths.is_empty() {
            for lib in &REQUIRED_LIBS {
                let library = pkg_config::Config::new()
                    .atleast_version("61.0")
                    .probe(lib)
                    .unwrap_or_else(|e| panic!("未能找到 {lib}: {e} "));
                include_paths.extend(library.include_paths);
            }
        }

        include_paths.sort();
        include_paths.dedup();

        let mut builder = utils::create_base_bindings();

        for path in include_paths {
            builder = builder.clang_arg(format!("-I{}", path.display()));
        }

        let bindings = builder.generate().expect("无法生成 FFmpeg 绑定");
        bindings
            .write_to_file(out_dir.join("bindings.rs"))
            .expect("无法写入 bindings.rs");
    }
}

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let slim_zip = manifest_dir.join("vendor").join("ffmpeg_slim.zip");
    let vendor_ffmpeg = manifest_dir.join("vendor").join("ffmpeg_slim");
    let vendor_configs = manifest_dir.join("vendor").join("configs");

    if slim_zip.exists() || (vendor_ffmpeg.exists() && vendor_configs.exists()) {
        bundled::build(&manifest_dir, &out_dir);
    } else {
        system::build(&out_dir, &target_os);
    }
}
