#![deny(clippy::unwrap_used)]

use anyhow::Result;
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use crate::{code_language::CodeLanguage, path_utils::make_absolute, source_file::SourceFile};

#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub struct GCCArgs {
    pub sources: Vec<SourceFile>,
    pub primary_output: Option<PathBuf>,
    pub user_includes: Vec<PathBuf>,
    pub system_includes: Vec<PathBuf>,
    pub defines: Vec<String>,
    pub warnings: Vec<String>,
    pub machine_args: Vec<String>,
    pub pipe: bool,
    pub f_flags: Vec<String>,
    pub g_flags: Vec<String>,
    pub opt_flags: Vec<String>,
    pub lang_std: Option<String>,
    pub depfile_generate: bool,
    pub depfile_output_path: Option<PathBuf>,
    pub depfile_target_name: Option<String>,
    pub stop_before_link: bool,
    pub stop_before_assemble: bool,
    pub stop_after_preprocessing: bool,
    pub shared: bool,
    pub libraries: Vec<String>,
    pub no_pie: bool,
    pub link_dirs: Vec<PathBuf>,
    pub linker_args: Vec<OsString>,
    pub print_sysroot: bool,
    pub flag_v: bool,
    pub include_files: Vec<SourceFile>,
    pub aa_flag: bool,
    pub target_flags: Vec<String>,
    pub cxx_flag: bool,
    pub ecxx_flag: bool,
    pub openmp_flag: bool,
    pub use_link_group: bool,
    pub preprocess_keep_defines: bool,
}

impl GCCArgs {
    pub fn parse<S: AsRef<OsStr>>(cwd: &Path, raw_args: &[S]) -> Result<Self> {
        let mut args = Self::default();

        let mut last_language = None;

        let mut raw_args_iter = raw_args.iter();
        while let Some(raw_arg) = raw_args_iter.next() {
            let arg_str = raw_arg.as_ref().to_str().ok_or_else(|| {
                anyhow::anyhow!(
                    "Failed to convert OsStr to str for arg: {}",
                    raw_arg.as_ref().to_string_lossy()
                )
            })?;
            if let Some(definition) = arg_str.strip_prefix("-D") {
                args.defines.push(definition.to_string());
            } else if let Some(path) = arg_str.strip_prefix("-I") {
                args.user_includes.push(make_absolute(cwd, Path::new(path)));
            } else if arg_str.starts_with("-isystem") {
                let path = raw_args_iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Missing path after -isystem"))?;
                args.system_includes
                    .push(make_absolute(cwd, Path::new(path)));
            } else if let Some(linker_arg) = arg_str.strip_prefix("-Wl,") {
                let mut arg_split = linker_arg.split(",");
                for linker_arg in arg_split.by_ref() {
                    args.linker_args.push(linker_arg.into());
                }
            } else if arg_str.starts_with("-W") {
                args.warnings.push(arg_str.to_string());
            } else if arg_str.starts_with("--target") {
                args.target_flags.push(arg_str.to_string());
            } else if arg_str.starts_with("-m") {
                args.machine_args.push(arg_str.to_string());
            } else if arg_str == "-pipe" {
                args.pipe = true;
            } else if arg_str == "-shared" {
                args.shared = true;
            } else if arg_str == "-print-sysroot" {
                args.print_sysroot = true;
            } else if arg_str == "-v" {
                args.flag_v = true;
            } else if arg_str == "-Aa" {
                args.aa_flag = true;
            } else if arg_str == "--c++" {
                args.cxx_flag = true;
            } else if arg_str == "--ec++" {
                args.ecxx_flag = true;
            } else if arg_str == "--openmp" {
                args.openmp_flag = true;
            } else if arg_str == "-dD" {
                args.preprocess_keep_defines = true;
            } else if arg_str == "-x" {
                let name = raw_args_iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Missing name"))?
                    .as_ref();
                let name = name.to_string_lossy().to_string();
                last_language = CodeLanguage::from_gcc_x_arg(&name)?;
            } else if arg_str == "-include" {
                let path = raw_args_iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Missing path for -include flag"))?;
                args.include_files.push(SourceFile {
                    path: make_absolute(cwd, Path::new(path)),
                    language_override: last_language,
                });
            } else if arg_str.starts_with("-f") {
                args.f_flags.push(arg_str.to_string());
            } else if arg_str.starts_with("-O") {
                args.opt_flags.push(arg_str.to_string());
            } else if arg_str.starts_with("-std=") {
                args.lang_std = Some(arg_str.to_string());
            } else if arg_str == "-MD" {
                args.depfile_generate = true;
            } else if arg_str == "-MT" {
                let name = raw_args_iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Missing name"))?
                    .as_ref();
                let name = name.to_string_lossy().to_string();
                args.depfile_target_name = Some(name);
            } else if arg_str == "-MF" {
                let path = raw_args_iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Missing path for -MF flag"))?;
                args.depfile_output_path = Some(make_absolute(cwd, Path::new(path)));
            } else if arg_str == "-o" {
                let path = raw_args_iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Missing path for -o flag"))?;
                args.primary_output = Some(make_absolute(cwd, Path::new(path)));
            } else if arg_str == "-c" {
                args.stop_before_link = true;
            } else if arg_str == "-S" {
                args.stop_before_assemble = true;
            } else if arg_str == "-E" {
                args.stop_after_preprocessing = true;
            } else if arg_str.starts_with("-g") {
                args.g_flags.push(arg_str.to_string());
            } else if let Some(library) = arg_str.strip_prefix("-l") {
                args.libraries.push(library.to_string());
            } else if arg_str == "-no-pie" {
                args.no_pie = true;
            } else if let Some(link_dir) = arg_str.strip_prefix("-L") {
                args.link_dirs.push(make_absolute(cwd, Path::new(link_dir)));
            } else if arg_str.starts_with("-Xlinker") {
                let arg = raw_args_iter.next().ok_or_else(|| {
                    anyhow::anyhow!("Missing argument for -Xlinker flag: {}", arg_str)
                })?;
                args.linker_args.push(arg.into());
            } else if arg_str.starts_with("-") {
                return Err(anyhow::anyhow!("Unknown GCC flag: {}", arg_str));
            } else {
                let path = make_absolute(cwd, Path::new(raw_arg));
                args.sources.push(SourceFile {
                    path,
                    language_override: last_language,
                });
            }
        }
        Ok(args)
    }

    pub fn to_args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![];
        if self.stop_before_link {
            args.push("-c".into());
        }
        if self.stop_before_assemble {
            args.push("-S".into());
        }
        if self.stop_after_preprocessing {
            args.push("-E".into());
        }
        if self.pipe {
            args.push("-pipe".into());
        }
        if self.shared {
            args.push("-shared".into());
        }
        if self.no_pie {
            args.push("-no-pie".into());
        }
        if self.preprocess_keep_defines {
            args.push("-dD".into());
        }
        if self.print_sysroot {
            args.push("-print-sysroot".into());
        }
        for flag in &self.target_flags {
            args.push(flag.into());
        }
        if self.aa_flag {
            args.push("-Aa".into());
        }
        if self.flag_v {
            args.push("-v".into());
        }
        if self.cxx_flag {
            args.push("--c++".into());
        }
        if self.ecxx_flag {
            args.push("--ec++".into());
        }
        if self.openmp_flag {
            args.push("--openmp".into());
        }
        if let Some(lang_std) = &self.lang_std {
            args.push(lang_std.into());
        }
        for arg in &self.opt_flags {
            args.push(arg.into());
        }
        for arg in &self.g_flags {
            args.push(arg.into());
        }
        for arg in &self.f_flags {
            args.push(arg.into());
        }
        for arg in &self.libraries {
            let mut combined = OsString::from("-l");
            combined.push(arg);
            args.push(combined);
        }
        if self.depfile_generate {
            args.push("-MD".into());
        }
        if let Some(name) = &self.depfile_target_name {
            args.push("-MT".into());
            args.push(name.into());
        }
        if let Some(path) = &self.depfile_output_path {
            args.push("-MF".into());
            args.push(path.as_os_str().into());
        }
        for arg in &self.warnings {
            args.push(arg.into());
        }
        for arg in &self.machine_args {
            args.push(arg.into());
        }
        for arg in &self.defines {
            let mut combined = OsString::from("-D");
            combined.push(arg);
            args.push(combined);
        }
        for arg in &self.user_includes {
            let mut combined = OsString::from("-I");
            combined.push(arg.as_os_str());
            args.push(combined);
        }
        for arg in &self.system_includes {
            args.push("-isystem".into());
            args.push(arg.as_os_str().into());
        }
        for arg in &self.link_dirs {
            let mut combined = OsString::from("-L");
            combined.push(arg.as_os_str());
            args.push(combined);
        }
        for arg in &self.linker_args {
            args.push("-Xlinker".into());
            args.push(arg.clone());
        }

        let mut last_language = None;

        for arg in &self.include_files {
            self.to_args_update_language(&mut last_language, &arg.language_override, &mut args);
            args.push("-include".into());
            args.push(arg.path.as_os_str().into());
        }
        if let Some(path) = &self.primary_output {
            args.push("-o".into());
            args.push(path.as_os_str().into());
        }
        if self.use_link_group {
            args.push("-Wl,--start-group".into());
        }
        for arg in &self.sources {
            self.to_args_update_language(&mut last_language, &arg.language_override, &mut args);
            args.push(arg.path.as_os_str().into());
        }
        if self.use_link_group {
            args.push("-Wl,--end-group".into());
        }
        args
    }

    fn to_args_update_language(
        &self,
        last_language: &mut Option<CodeLanguage>,
        new_language: &Option<CodeLanguage>,
        args: &mut Vec<OsString>,
    ) {
        if last_language != new_language {
            args.push("-x".into());
            if let Some(language) = &new_language {
                args.push(language.to_gcc_x_arg().into());
            } else {
                args.push("none".into());
            }
            *last_language = *new_language;
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_gcc_compile_args_for_compilation() {
        let raw_args = vec![
            "-DHAVE_EXECINFO_H",
            "-DHAVE_MALLOC_STATS_H",
            "-DNDEBUG",
            "-DWITH_ASSERT_ABORT",
            "-DWITH_DNA_GHASH",
            "-DWITH_FREESTYLE",
            "-DWITH_GHOST_WAYLAND_LIBDECOR",
            "-DWITH_TBB",
            "-D_FILE_OFFSET_BITS=64",
            "-D_LARGEFILE64_SOURCE",
            "-D_LARGEFILE_SOURCE",
            "-D__LITTLE_ENDIAN__",
            "-I/home/jacques/blender/blender/source/blender/makesdna",
            "-I/home/jacques/Documents/ccelerate_test/build_blender/source/blender/makesdna/intern",
            "-I/home/jacques/blender/blender/source/blender/blenlib",
            "-I/home/jacques/blender/blender/source/blender/imbuf",
            "-I/home/jacques/blender/blender/source/blender/imbuf/movie",
            "-I/home/jacques/blender/blender/intern/atomic/.",
            "-I/home/jacques/blender/blender/intern/guardedalloc",
            "-I/home/jacques/blender/blender/extern/fmtlib/include",
            "-isystem",
            "/home/jacques/blender/blender/lib/linux_x64/tbb/include",
            "-Wuninitialized",
            "-Wredundant-decls",
            "-Wall",
            "-Wno-invalid-offsetof",
            "-Wno-sign-compare",
            "-Wlogical-op",
            "-Winit-self",
            "-Wmissing-include-dirs",
            "-Wno-div-by-zero",
            "-Wtype-limits",
            "-Werror=return-type",
            "-Wno-char-subscripts",
            "-Wno-unknown-pragmas",
            "-Wpointer-arith",
            "-Wunused-parameter",
            "-Wwrite-strings",
            "-Wundef",
            "-Wcomma-subscript",
            "-Wformat-signedness",
            "-Wrestrict",
            "-Wno-suggest-override",
            "-Wuninitialized",
            "-Wno-stringop-overread",
            "-Wno-stringop-overflow",
            "-Wimplicit-fallthrough=5",
            "-Wundef",
            "-Wmissing-declarations",
            "-march=x86-64-v2",
            "-pipe",
            "-fPIC",
            "-funsigned-char",
            "-fno-strict-aliasing",
            "-ffp-contract=off",
            "-fmacro-prefix-map=/home/jacques/blender/blender/=",
            "-fmacro-prefix-map=/home/jacques/Documents/ccelerate_test/build_blender/=",
            "-Wno-maybe-uninitialized",
            "-O2",
            "-DNDEBUG",
            "-std=c++17",
            "-fopenmp",
            "-MD",
            "-MT",
            "source/blender/makesdna/intern/CMakeFiles/makesdna.dir/__/__/blenlib/intern/BLI_mempool.cc.o",
            "-MF",
            "source/blender/makesdna/intern/CMakeFiles/makesdna.dir/__/__/blenlib/intern/BLI_mempool.cc.o.d",
            "-o",
            "source/blender/makesdna/intern/CMakeFiles/makesdna.dir/__/__/blenlib/intern/BLI_mempool.cc.o",
            "-c",
            "/home/jacques/blender/blender/source/blender/blenlib/intern/BLI_mempool.cc",
        ];
        let raw_args: Vec<&OsStr> = raw_args.iter().map(|s| s.as_ref()).collect();

        let args = GCCArgs::parse(
            Path::new("/home/jacques/Documents/ccelerate_test/build_blender"),
            &raw_args,
        )
        .expect("should be valid");
        assert_eq!(
            args.sources,
            vec![SourceFile {
                path: PathBuf::from(
                    "/home/jacques/blender/blender/source/blender/blenlib/intern/BLI_mempool.cc"
                ),
                language_override: None
            }]
        );
        assert_eq!(
            args.primary_output,
            Some(PathBuf::from(
                "/home/jacques/Documents/ccelerate_test/build_blender/source/blender/makesdna/intern/CMakeFiles/makesdna.dir/__/__/blenlib/intern/BLI_mempool.cc.o"
            ))
        );
        assert!(args.stop_before_link);
        assert!(args.depfile_generate);
        assert_eq!(
        args.depfile_target_name,
        Some("source/blender/makesdna/intern/CMakeFiles/makesdna.dir/__/__/blenlib/intern/BLI_mempool.cc.o".to_string())
    );
        assert_eq!(
            args.depfile_output_path,
            Some(PathBuf::from(
                "/home/jacques/Documents/ccelerate_test/build_blender/source/blender/makesdna/intern/CMakeFiles/makesdna.dir/__/__/blenlib/intern/BLI_mempool.cc.o.d"
            ))
        );
        assert_eq!(
        args.user_includes,
        ["/home/jacques/blender/blender/source/blender/makesdna",
         "/home/jacques/Documents/ccelerate_test/build_blender/source/blender/makesdna/intern",
            "/home/jacques/blender/blender/source/blender/blenlib",
            "/home/jacques/blender/blender/source/blender/imbuf",
            "/home/jacques/blender/blender/source/blender/imbuf/movie",
            "/home/jacques/blender/blender/intern/atomic/.",
            "/home/jacques/blender/blender/intern/guardedalloc",
            "/home/jacques/blender/blender/extern/fmtlib/include"]
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>()
    );
        assert_eq!(
            args.system_includes,
            vec![PathBuf::from(
                "/home/jacques/blender/blender/lib/linux_x64/tbb/include"
            )]
        );
        assert_eq!(
            args.defines,
            vec![
                "HAVE_EXECINFO_H",
                "HAVE_MALLOC_STATS_H",
                "NDEBUG",
                "WITH_ASSERT_ABORT",
                "WITH_DNA_GHASH",
                "WITH_FREESTYLE",
                "WITH_GHOST_WAYLAND_LIBDECOR",
                "WITH_TBB",
                "_FILE_OFFSET_BITS=64",
                "_LARGEFILE64_SOURCE",
                "_LARGEFILE_SOURCE",
                "__LITTLE_ENDIAN__",
                "NDEBUG",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
        );
        assert_eq!(args.opt_flags, vec!["-O2".to_string()]);
        assert_eq!(args.machine_args, vec!["-march=x86-64-v2".to_string()]);

        test_round_trip(&raw_args);
    }

    #[test]
    fn test_parse_gcc_compile_args_for_so_linking() {
        let raw_args = vec![
            "-fPIC",
            "-Wuninitialized",
            "-Wall",
            "-Wno-invalid-offsetof",
            "-Wno-sign-compare",
            "-Wlogical-op",
            "-Winit-self",
            "-Wmissing-include-dirs",
            "-Wno-div-by-zero",
            "-Wtype-limits",
            "-Wno-char-subscripts",
            "-Wno-unknown-pragmas",
            "-Wpointer-arith",
            "-Wcomma-subscript",
            "-Wformat-signedness",
            "-Wrestrict",
            "-Wno-suggest-override",
            "-Wuninitialized",
            "-Wno-stringop-overread",
            "-Wno-stringop-overflow",
            "-Wimplicit-fallthrough=5",
            "-march=x86-64-v2",
            "-pipe",
            "-fPIC",
            "-funsigned-char",
            "-fno-strict-aliasing",
            "-ffp-contract=off",
            "-fmacro-prefix-map=/home/jacques/blender/blender/=",
            "-fmacro-prefix-map=/home/jacques/Documents/ccelerate_test/build_blender/=",
            "-Wno-deprecated-declarations",
            "-Wno-unused-parameter",
            "-Wno-unused-function",
            "-Wno-type-limits",
            "-Wno-int-in-bool-context",
            "-Wno-format",
            "-Wno-switch",
            "-Wno-unused-variable",
            "-Wno-uninitialized",
            "-Wno-implicit-fallthrough",
            "-Wno-error=unused-but-set-variable",
            "-Wno-class-memaccess",
            "-Wno-comment",
            "-Wno-unused-local-typedefs",
            "-Wno-unused-variable",
            "-Wno-uninitialized",
            "-Wno-maybe-uninitialized",
            "-O2",
            "-DNDEBUG",
            "-shared",
            "-Wl,-soname,libextern_draco.so",
            "-o",
            "lib/libextern_draco.so",
            "extern/draco/CMakeFiles/extern_draco.dir/src/common.cpp.o",
            "extern/draco/CMakeFiles/extern_draco.dir/src/decoder.cpp.o",
            "extern/draco/CMakeFiles/extern_draco.dir/src/encoder.cpp.o",
            "-Wl,-rpath,$ORIGIN/lib:/home/jacques/Documents/ccelerate_test/build_blender/bin/lib",
            "lib/libdraco.a",
        ];
        let raw_args: Vec<&OsStr> = raw_args.iter().map(|s| s.as_ref()).collect();

        let args = GCCArgs::parse(
            Path::new("/home/jacques/Documents/ccelerate_test/build_blender"),
            &raw_args,
        )
        .expect("should be valid");

        assert!(args.shared);
        assert_eq!(
        args.sources,
        ["/home/jacques/Documents/ccelerate_test/build_blender/extern/draco/CMakeFiles/extern_draco.dir/src/common.cpp.o",
            "/home/jacques/Documents/ccelerate_test/build_blender/extern/draco/CMakeFiles/extern_draco.dir/src/decoder.cpp.o",
            "/home/jacques/Documents/ccelerate_test/build_blender/extern/draco/CMakeFiles/extern_draco.dir/src/encoder.cpp.o",
            "/home/jacques/Documents/ccelerate_test/build_blender/lib/libdraco.a"].iter().map(|s| SourceFile {
            path: Path::new(s).to_path_buf(),
            language_override: None
        }).collect::<Vec<_>>()
    );
        assert!(!args.stop_before_link);
        assert!(!args.depfile_generate);
        assert_eq!(
            args.primary_output,
            Some(PathBuf::from(
                "/home/jacques/Documents/ccelerate_test/build_blender/lib/libextern_draco.so"
            ))
        );
        assert_eq!(args.defines, vec!["NDEBUG".to_string()]);

        test_round_trip(&raw_args);
    }

    #[test]
    fn test_parse_gcc_compile_args_for_final_linking() {
        let raw_args = vec![
            "-Wuninitialized",
            "-Wredundant-decls",
            "-Wall",
            "-Wno-invalid-offsetof",
            "-Wno-sign-compare",
            "-Wlogical-op",
            "-Winit-self",
            "-Wmissing-include-dirs",
            "-Wno-div-by-zero",
            "-Wtype-limits",
            "-Werror=return-type",
            "-Wno-char-subscripts",
            "-Wno-unknown-pragmas",
            "-Wpointer-arith",
            "-Wunused-parameter",
            "-Wwrite-strings",
            "-Wundef",
            "-Wcomma-subscript",
            "-Wformat-signedness",
            "-Wrestrict",
            "-Wno-suggest-override",
            "-Wuninitialized",
            "-Wno-stringop-overread",
            "-Wno-stringop-overflow",
            "-Wimplicit-fallthrough=5",
            "-Wundef",
            "-Wmissing-declarations",
            "-march=x86-64-v2",
            "-pipe",
            "-fPIC",
            "-funsigned-char",
            "-fno-strict-aliasing",
            "-ffp-contract=off",
            "-fmacro-prefix-map=/home/jacques/blender/blender/=",
            "-fmacro-prefix-map=/home/jacques/Documents/ccelerate_test/build_blender/=",
            "-Wno-maybe-uninitialized",
            "-O2",
            "-DNDEBUG",
            "-no-pie",
            "-Wl,--version-script=/home/jacques/blender/blender/source/creator/symbols_unix.map",
            "-latomic",
            "source/blender/blentranslation/msgfmt/CMakeFiles/msgfmt.dir/msgfmt.cc.o",
            "-o",
            "bin/msgfmt",
            "-Wl,-rpath,$ORIGIN/lib:/home/jacques/Documents/ccelerate_test/build_blender/bin/lib:/home/jacques/blender/blender/lib/linux_x64/tbb/lib",
            "lib/libbf_blenlib.a",
            "lib/libbf_intern_guardedalloc.a",
            "/home/jacques/blender/blender/lib/linux_x64/zlib/lib/libz.a",
            "-lutil",
            "-lc",
            "-lm",
            "-ldl",
            "lib/libbf_dna.a",
            "lib/libbf_intern_guardedalloc.a",
            "lib/libextern_fmtlib.a",
            "lib/libextern_xxhash.a",
            "lib/libbf_intern_eigen.a",
            "/usr/lib/gcc/x86_64-redhat-linux/14/libgomp.a",
            "lib/libextern_wcwidth.a",
            "/home/jacques/blender/blender/lib/linux_x64/tbb/lib/libtbb.so",
            "/home/jacques/blender/blender/lib/linux_x64/zstd/lib/libzstd.a",
            "/home/jacques/blender/blender/lib/linux_x64/gmp/lib/libgmpxx.a",
            "/home/jacques/blender/blender/lib/linux_x64/gmp/lib/libgmp.a",
            "/home/jacques/blender/blender/lib/linux_x64/fftw3/lib/libfftw3f.a",
            "/home/jacques/blender/blender/lib/linux_x64/fftw3/lib/libfftw3.a",
            "/home/jacques/blender/blender/lib/linux_x64/fftw3/lib/libfftw3f_threads.a",
            "lib/libbf_intern_libc_compat.a",
        ];
        let raw_args: Vec<&OsStr> = raw_args.iter().map(|s| s.as_ref()).collect();

        let args = GCCArgs::parse(
            Path::new("/home/jacques/Documents/ccelerate_test/build_blender"),
            &raw_args,
        )
        .expect("should be valid");

        assert!(args.no_pie);
        assert_eq!(
            args.primary_output,
            Some(PathBuf::from(
                "/home/jacques/Documents/ccelerate_test/build_blender/bin/msgfmt"
            ))
        );
        assert_eq!(args.libraries, vec!["atomic", "util", "c", "m", "dl"]);
        assert_eq!(
            args.sources,
            vec![
                "/home/jacques/Documents/ccelerate_test/build_blender/source/blender/blentranslation/msgfmt/CMakeFiles/msgfmt.dir/msgfmt.cc.o",
                "/home/jacques/Documents/ccelerate_test/build_blender/lib/libbf_blenlib.a",
                "/home/jacques/Documents/ccelerate_test/build_blender/lib/libbf_intern_guardedalloc.a",
                "/home/jacques/blender/blender/lib/linux_x64/zlib/lib/libz.a",
                "/home/jacques/Documents/ccelerate_test/build_blender/lib/libbf_dna.a",
                "/home/jacques/Documents/ccelerate_test/build_blender/lib/libbf_intern_guardedalloc.a",
                "/home/jacques/Documents/ccelerate_test/build_blender/lib/libextern_fmtlib.a",
                "/home/jacques/Documents/ccelerate_test/build_blender/lib/libextern_xxhash.a",
                "/home/jacques/Documents/ccelerate_test/build_blender/lib/libbf_intern_eigen.a",
                "/usr/lib/gcc/x86_64-redhat-linux/14/libgomp.a",
                "/home/jacques/Documents/ccelerate_test/build_blender/lib/libextern_wcwidth.a",
                "/home/jacques/blender/blender/lib/linux_x64/tbb/lib/libtbb.so",
                "/home/jacques/blender/blender/lib/linux_x64/zstd/lib/libzstd.a",
                "/home/jacques/blender/blender/lib/linux_x64/gmp/lib/libgmpxx.a",
                "/home/jacques/blender/blender/lib/linux_x64/gmp/lib/libgmp.a",
                "/home/jacques/blender/blender/lib/linux_x64/fftw3/lib/libfftw3f.a",
                "/home/jacques/blender/blender/lib/linux_x64/fftw3/lib/libfftw3.a",
                "/home/jacques/blender/blender/lib/linux_x64/fftw3/lib/libfftw3f_threads.a",
                "/home/jacques/Documents/ccelerate_test/build_blender/lib/libbf_intern_libc_compat.a"
            ].iter().map(|s| SourceFile {
                path: Path::new(s).to_path_buf(),
                language_override: None
            }).collect::<Vec<_>>()
        );

        test_round_trip(&raw_args);
    }

    #[test]
    fn test_parse_gcc_compile_args_for_final_linking_blender() {
        let raw_args = vec![
            "-Wuninitialized",
            "-Wredundant-decls",
            "-Wall",
            "-Wno-invalid-offsetof",
            "-Wno-sign-compare",
            "-Wlogical-op",
            "-Winit-self",
            "-Wmissing-include-dirs",
            "-Wno-div-by-zero",
            "-Wtype-limits",
            "-Werror=return-type",
            "-Wno-char-subscripts",
            "-Wno-unknown-pragmas",
            "-Wpointer-arith",
            "-Wunused-parameter",
            "-Wwrite-strings",
            "-Wundef",
            "-Wcomma-subscript",
            "-Wformat-signedness",
            "-Wrestrict",
            "-Wno-suggest-override",
            "-Wuninitialized",
            "-Wno-stringop-overread",
            "-Wno-stringop-overflow",
            "-Wimplicit-fallthrough=5",
            "-Wundef",
            "-Wmissing-declarations",
            "-march=x86-64-v2",
            "-pipe",
            "-fPIC",
            "-funsigned-char",
            "-fno-strict-aliasing",
            "-ffp-contract=off",
            "-fmacro-prefix-map=/home/jacques/blender/blender/=",
            "-fmacro-prefix-map=/home/jacques/Documents/ccelerate_test/build_blender/=",
            "-Wno-maybe-uninitialized",
            "-O2",
            "-DNDEBUG",
            "-no-pie",
            "-Wl,--version-script=/home/jacques/blender/blender/source/creator/symbols_unix.map",
            "-latomic",
            "source/creator/CMakeFiles/blender.dir/creator.cc.o",
            "source/creator/CMakeFiles/blender.dir/creator_args.cc.o",
            "source/creator/CMakeFiles/blender.dir/creator_signals.cc.o",
            "source/creator/CMakeFiles/blender.dir/buildinfo.c.o",
            "-o",
            "bin/blender",
            "-L/home/jacques/blender/blender/lib/linux_x64/materialx/lib",
            "-Wl,-rpath,$ORIGIN/lib:/home/jacques/Documents/ccelerate_test/build_blender/bin/lib:/home/jacques/blender/blender/lib/linux_x64/materialx/lib:/home/jacques/blender/blender/lib/linux_x64/tbb/lib:/home/jacques/blender/blender/lib/linux_x64/opensubdiv/lib:/home/jacques/blender/blender/lib/linux_x64/vulkan/lib:/home/jacques/blender/blender/lib/linux_x64/openexr/lib:/home/jacques/blender/blender/lib/linux_x64/imath/lib:/home/jacques/blender/blender/lib/linux_x64/embree/lib:/home/jacques/blender/blender/lib/linux_x64/dpcpp/lib:/home/jacques/blender/blender/lib/linux_x64/opencolorio/lib:/home/jacques/blender/blender/lib/linux_x64/openimageio/lib:/home/jacques/blender/blender/lib/linux_x64/openimagedenoise/lib:/home/jacques/blender/blender/lib/linux_x64/osl/lib:/home/jacques/blender/blender/lib/linux_x64/usd/lib:/home/jacques/blender/blender/lib/linux_x64/openvdb/lib:",
            "bin/lib/libblender_cpu_check.so",
            "lib/libbf_blenkernel.a",
            "/home/jacques/blender/blender/lib/linux_x64/tbb/lib/libtbb.so",
            "lib/libbf_blenkernel.a",
            "lib/libbf_blenlib.a",
            "lib/libbf_bmesh.a",
            "lib/libbf_depsgraph.a",
            "lib/libbf_dna.a",
            "lib/libbf_gpu.a",
            "lib/libbf_imbuf.a",
            "lib/libbf_imbuf_movie.a",
            "lib/libbf_intern_clog.a",
            "lib/libbf_intern_guardedalloc.a",
            "lib/libbf_render.a",
            "lib/libbf_windowmanager.a",
            "/usr/lib/gcc/x86_64-redhat-linux/14/libgomp.a",
            "/home/jacques/blender/blender/lib/linux_x64/jemalloc/lib/libjemalloc.a",
            "-lutil",
            "-lc",
            "-lm",
            "-ldl",
            "lib/libbf_animrig.a",
            "lib/libbf_asset_system.a",
            "lib/libbf_blenfont.a",
            "lib/libbf_blenloader.a",
            "lib/libbf_blentranslation.a",
            "lib/libbf_draw.a",
            "lib/libbf_ikplugin.a",
            "lib/libbf_intern_ghost.a",
            "lib/libbf_modifiers.a",
            "lib/libbf_nodes.a",
            "lib/libbf_rna.a",
            "lib/libbf_sequencer.a",
            "lib/libbf_shader_fx.a",
            "lib/libbf_simulation.a",
            "lib/libbf_python.a",
            "lib/libbf_python_bmesh.a",
            "lib/libbf_imbuf_openimageio.a",
            "lib/libbf_intern_opencolorio.a",
            "lib/libbf_imbuf_openexr.a",
            "lib/libbf_imbuf_cineon.a",
            "lib/libbf_compositor.a",
            "lib/libbf_freestyle.a",
            "lib/libbf_editor_screen.a",
            "lib/libbf_editor_undo.a",
            "lib/libbf_geometry.a",
            "lib/libbf_io_alembic.a",
            "lib/libbf_io_usd.a",
            "lib/libbf_nodes_composite.a",
            "lib/libbf_nodes_function.a",
            "lib/libbf_nodes_geometry.a",
            "lib/libbf_nodes_shader.a",
            "lib/libbf_nodes_texture.a",
            "lib/libbf_editor_space_api.a",
            "lib/libbf_editor_animation.a",
            "lib/libbf_editor_armature.a",
            "lib/libbf_editor_asset.a",
            "lib/libbf_editor_curve.a",
            "lib/libbf_editor_curves.a",
            "lib/libbf_editor_gizmo_library.a",
            "lib/libbf_editor_gpencil_legacy.a",
            "lib/libbf_editor_io.a",
            "lib/libbf_editor_mesh.a",
            "lib/libbf_editor_object.a",
            "lib/libbf_editor_physics.a",
            "lib/libbf_editor_pointcloud.a",
            "lib/libbf_editor_render.a",
            "lib/libbf_editor_scene.a",
            "lib/libbf_editor_sculpt_paint.a",
            "lib/libbf_editor_sound.a",
            "lib/libbf_editor_transform.a",
            "lib/libbf_editor_interface.a",
            "lib/libbf_python_gpu.a",
            "lib/libbf_intern_cycles.a",
            "lib/libbf_render_hydra.a",
            "lib/libbf_python_mathutils.a",
            "lib/libbf_editor_space_sequencer.a",
            "lib/libbf_io_common.a",
            "lib/libbf_nodes_functions_generated.a",
            "lib/libbf_io_csv.a",
            "lib/libbf_io_stl.a",
            "lib/libbf_io_ply.a",
            "lib/libbf_io_wavefront_obj.a",
            "lib/libbf_nodes_geometry_generated.a",
            "lib/libbf_editor_geometry.a",
            "lib/libbf_editor_space_action.a",
            "lib/libbf_editor_space_buttons.a",
            "lib/libbf_editor_space_clip.a",
            "lib/libbf_editor_space_console.a",
            "lib/libbf_editor_space_file.a",
            "lib/libbf_editor_space_graph.a",
            "lib/libbf_editor_space_image.a",
            "lib/libbf_editor_space_info.a",
            "lib/libbf_editor_space_nla.a",
            "lib/libbf_editor_space_node.a",
            "lib/libbf_editor_space_outliner.a",
            "lib/libbf_editor_space_script.a",
            "lib/libbf_editor_space_spreadsheet.a",
            "lib/libbf_editor_space_statusbar.a",
            "lib/libbf_editor_space_text.a",
            "lib/libbf_editor_space_topbar.a",
            "lib/libbf_editor_space_userpref.a",
            "lib/libbf_editor_space_view3d.a",
            "lib/libbf_io_collada.a",
            "lib/libbf_io_grease_pencil.a",
            "lib/libbf_editor_metaball.a",
            "lib/libbf_editor_grease_pencil.a",
            "lib/libbf_editor_mask.a",
            "lib/libbf_editor_id_management.a",
            "lib/libbf_python_ext.a",
            "lib/libbf_editor_util.a",
            "lib/libbf_editor_uvedit.a",
            "lib/libbf_editor_lattice.a",
            "lib/libbf_blenkernel.a",
            "lib/libbf_bmesh.a",
            "lib/libbf_depsgraph.a",
            "lib/libbf_gpu.a",
            "lib/libbf_imbuf.a",
            "lib/libbf_imbuf_movie.a",
            "lib/libbf_render.a",
            "lib/libbf_windowmanager.a",
            "lib/libbf_animrig.a",
            "lib/libbf_asset_system.a",
            "lib/libbf_blenfont.a",
            "lib/libbf_blenloader.a",
            "lib/libbf_blentranslation.a",
            "lib/libbf_draw.a",
            "lib/libbf_ikplugin.a",
            "lib/libbf_intern_ghost.a",
            "lib/libbf_modifiers.a",
            "lib/libbf_nodes.a",
            "lib/libbf_rna.a",
            "lib/libbf_sequencer.a",
            "lib/libbf_shader_fx.a",
            "lib/libbf_simulation.a",
            "lib/libbf_python.a",
            "lib/libbf_python_bmesh.a",
            "lib/libbf_imbuf_openimageio.a",
            "lib/libbf_intern_opencolorio.a",
            "lib/libbf_imbuf_openexr.a",
            "lib/libbf_imbuf_cineon.a",
            "lib/libbf_compositor.a",
            "lib/libbf_freestyle.a",
            "lib/libbf_editor_screen.a",
            "lib/libbf_editor_undo.a",
            "lib/libbf_geometry.a",
            "lib/libbf_io_alembic.a",
            "lib/libbf_io_usd.a",
            "lib/libbf_nodes_composite.a",
            "lib/libbf_nodes_function.a",
            "lib/libbf_nodes_geometry.a",
            "lib/libbf_nodes_shader.a",
            "lib/libbf_nodes_texture.a",
            "lib/libbf_editor_space_api.a",
            "lib/libbf_editor_animation.a",
            "lib/libbf_editor_armature.a",
            "lib/libbf_editor_asset.a",
            "lib/libbf_editor_curve.a",
            "lib/libbf_editor_curves.a",
            "lib/libbf_editor_gizmo_library.a",
            "lib/libbf_editor_gpencil_legacy.a",
            "lib/libbf_editor_io.a",
            "lib/libbf_editor_mesh.a",
            "lib/libbf_editor_object.a",
            "lib/libbf_editor_physics.a",
            "lib/libbf_editor_pointcloud.a",
            "lib/libbf_editor_render.a",
            "lib/libbf_editor_scene.a",
            "lib/libbf_editor_sculpt_paint.a",
            "lib/libbf_editor_sound.a",
            "lib/libbf_editor_transform.a",
            "lib/libbf_editor_interface.a",
            "lib/libbf_python_gpu.a",
            "lib/libbf_intern_cycles.a",
            "lib/libbf_render_hydra.a",
            "lib/libbf_python_mathutils.a",
            "lib/libbf_editor_space_sequencer.a",
            "lib/libbf_io_common.a",
            "lib/libbf_nodes_functions_generated.a",
            "lib/libbf_io_csv.a",
            "lib/libbf_io_stl.a",
            "lib/libbf_io_ply.a",
            "lib/libbf_io_wavefront_obj.a",
            "lib/libbf_nodes_geometry_generated.a",
            "lib/libbf_editor_geometry.a",
            "lib/libbf_editor_space_action.a",
            "lib/libbf_editor_space_buttons.a",
            "lib/libbf_editor_space_clip.a",
            "lib/libbf_editor_space_console.a",
            "lib/libbf_editor_space_file.a",
            "lib/libbf_editor_space_graph.a",
            "lib/libbf_editor_space_image.a",
            "lib/libbf_editor_space_info.a",
            "lib/libbf_editor_space_nla.a",
            "lib/libbf_editor_space_node.a",
            "lib/libbf_editor_space_outliner.a",
            "lib/libbf_editor_space_script.a",
            "lib/libbf_editor_space_spreadsheet.a",
            "lib/libbf_editor_space_statusbar.a",
            "lib/libbf_editor_space_text.a",
            "lib/libbf_editor_space_topbar.a",
            "lib/libbf_editor_space_userpref.a",
            "lib/libbf_editor_space_view3d.a",
            "lib/libbf_io_collada.a",
            "lib/libbf_io_grease_pencil.a",
            "lib/libbf_editor_metaball.a",
            "lib/libbf_editor_grease_pencil.a",
            "lib/libbf_editor_mask.a",
            "lib/libbf_editor_id_management.a",
            "lib/libbf_python_ext.a",
            "lib/libbf_editor_util.a",
            "lib/libbf_editor_uvedit.a",
            "lib/libbf_editor_lattice.a",
            "lib/libbf_blenkernel.a",
            "lib/libbf_bmesh.a",
            "lib/libbf_depsgraph.a",
            "lib/libbf_gpu.a",
            "lib/libbf_imbuf.a",
            "lib/libbf_imbuf_movie.a",
            "lib/libbf_render.a",
            "lib/libbf_windowmanager.a",
            "lib/libbf_animrig.a",
            "lib/libbf_asset_system.a",
            "lib/libbf_blenfont.a",
            "lib/libbf_blenloader.a",
            "lib/libbf_blentranslation.a",
            "lib/libbf_draw.a",
            "lib/libbf_ikplugin.a",
            "lib/libbf_intern_ghost.a",
            "lib/libbf_modifiers.a",
            "lib/libbf_nodes.a",
            "lib/libbf_rna.a",
            "lib/libbf_sequencer.a",
            "lib/libbf_shader_fx.a",
            "lib/libbf_simulation.a",
            "lib/libbf_python.a",
            "lib/libbf_python_bmesh.a",
            "lib/libbf_imbuf_openimageio.a",
            "lib/libbf_intern_opencolorio.a",
            "lib/libbf_imbuf_openexr.a",
            "lib/libbf_imbuf_cineon.a",
            "lib/libbf_compositor.a",
            "lib/libbf_freestyle.a",
            "lib/libbf_editor_screen.a",
            "lib/libbf_editor_undo.a",
            "lib/libbf_geometry.a",
            "lib/libbf_io_alembic.a",
            "lib/libbf_io_usd.a",
            "lib/libbf_nodes_composite.a",
            "lib/libbf_nodes_function.a",
            "lib/libbf_nodes_geometry.a",
            "lib/libbf_nodes_shader.a",
            "lib/libbf_nodes_texture.a",
            "lib/libbf_editor_space_api.a",
            "lib/libbf_editor_animation.a",
            "lib/libbf_editor_armature.a",
            "lib/libbf_editor_asset.a",
            "lib/libbf_editor_curve.a",
            "lib/libbf_editor_curves.a",
            "lib/libbf_editor_gizmo_library.a",
            "lib/libbf_editor_gpencil_legacy.a",
            "lib/libbf_editor_io.a",
            "lib/libbf_editor_mesh.a",
            "lib/libbf_editor_object.a",
            "lib/libbf_editor_physics.a",
            "lib/libbf_editor_pointcloud.a",
            "lib/libbf_editor_render.a",
            "lib/libbf_editor_scene.a",
            "lib/libbf_editor_sculpt_paint.a",
            "lib/libbf_editor_sound.a",
            "lib/libbf_editor_transform.a",
            "lib/libbf_editor_interface.a",
            "lib/libbf_python_gpu.a",
            "lib/libbf_intern_cycles.a",
            "lib/libbf_render_hydra.a",
            "lib/libbf_python_mathutils.a",
            "lib/libbf_editor_space_sequencer.a",
            "lib/libbf_io_common.a",
            "lib/libbf_nodes_functions_generated.a",
            "lib/libbf_io_csv.a",
            "lib/libbf_io_stl.a",
            "lib/libbf_io_ply.a",
            "lib/libbf_io_wavefront_obj.a",
            "lib/libbf_nodes_geometry_generated.a",
            "lib/libbf_editor_geometry.a",
            "lib/libbf_editor_space_action.a",
            "lib/libbf_editor_space_buttons.a",
            "lib/libbf_editor_space_clip.a",
            "lib/libbf_editor_space_console.a",
            "lib/libbf_editor_space_file.a",
            "lib/libbf_editor_space_graph.a",
            "lib/libbf_editor_space_image.a",
            "lib/libbf_editor_space_info.a",
            "lib/libbf_editor_space_nla.a",
            "lib/libbf_editor_space_node.a",
            "lib/libbf_editor_space_outliner.a",
            "lib/libbf_editor_space_script.a",
            "lib/libbf_editor_space_spreadsheet.a",
            "lib/libbf_editor_space_statusbar.a",
            "lib/libbf_editor_space_text.a",
            "lib/libbf_editor_space_topbar.a",
            "lib/libbf_editor_space_userpref.a",
            "lib/libbf_editor_space_view3d.a",
            "lib/libbf_io_collada.a",
            "lib/libbf_io_grease_pencil.a",
            "lib/libbf_editor_metaball.a",
            "lib/libbf_editor_grease_pencil.a",
            "lib/libbf_editor_mask.a",
            "lib/libbf_editor_id_management.a",
            "lib/libbf_python_ext.a",
            "lib/libbf_editor_util.a",
            "lib/libbf_editor_uvedit.a",
            "lib/libbf_editor_lattice.a",
            "lib/libbf_intern_libmv.a",
            "lib/libextern_ceres.a",
            "/home/jacques/blender/blender/lib/linux_x64/png/lib/libpng.a",
            "-lm",
            "lib/libbf_intern_opensubdiv.a",
            "lib/libextern_binreloc.a",
            "lib/libbf_intern_rigidbody.a",
            "lib/libextern_minilzo.a",
            "lib/libextern_lzma.a",
            "/home/jacques/blender/blender/lib/linux_x64/opensubdiv/lib/libosdGPU.so",
            "/home/jacques/blender/blender/lib/linux_x64/opensubdiv/lib/libosdCPU.so",
            "lib/libbf_intern_quadriflow.a",
            "lib/libextern_quadriflow.a",
            "lib/libextern_rangetree.a",
            "/home/jacques/blender/blender/lib/linux_x64/shaderc/lib/libshaderc_combined.a",
            "lib/libextern_vulkan_memory_allocator.a",
            "lib/libbf_gpu_shaders.a",
            "-lrt",
            "/home/jacques/blender/blender/lib/linux_x64/jpeg/lib/libjpeg.a",
            "/home/jacques/blender/blender/lib/linux_x64/webp/lib/libwebp.a",
            "/home/jacques/blender/blender/lib/linux_x64/webp/lib/libwebpmux.a",
            "/home/jacques/blender/blender/lib/linux_x64/webp/lib/libwebpdemux.a",
            "/home/jacques/blender/blender/lib/linux_x64/webp/lib/libsharpyuv.a",
            "/home/jacques/blender/blender/lib/linux_x64/openjpeg/lib/libopenjp2.a",
            "/home/jacques/blender/blender/lib/linux_x64/freetype/lib/libfreetype.a",
            "/home/jacques/blender/blender/lib/linux_x64/brotli/lib/libbrotlidec-static.a",
            "/home/jacques/blender/blender/lib/linux_x64/brotli/lib/libbrotlicommon-static.a",
            "lib/libbf_intern_memutil.a",
            "lib/libbf_draw_shaders.a",
            "lib/libbf_intern_iksolver.a",
            "lib/libbf_intern_itasc.a",
            "/home/jacques/blender/blender/lib/linux_x64/vulkan/lib/libvulkan.so",
            "/home/jacques/blender/blender/lib/linux_x64/spnav/lib/libspnav.a",
            "/usr/lib64/libX11.so",
            "/usr/lib64/libXrender.so",
            "lib/libextern_xdnd.a",
            "/usr/lib64/libXxf86vm.so",
            "/usr/lib64/libXfixes.so",
            "/usr/lib64/libXi.so",
            "/usr/lib64/libxkbcommon.so",
            "lib/libbf_intern_wayland_dynload.a",
            "/home/jacques/blender/blender/lib/linux_x64/xr_openxr_sdk/lib/libopenxr_loader.a",
            "lib/libbf_intern_dualcon.a",
            "lib/libbf_ocio_shaders.a",
            "lib/libbf_compositor_shaders.a",
            "/home/jacques/blender/blender/lib/linux_x64/openexr/lib/libIex.so",
            "/home/jacques/blender/blender/lib/linux_x64/openexr/lib/libOpenEXR.so",
            "/home/jacques/blender/blender/lib/linux_x64/openexr/lib/libOpenEXRCore.so",
            "/home/jacques/blender/blender/lib/linux_x64/openexr/lib/libIlmThread.so",
            "/home/jacques/blender/blender/lib/linux_x64/imath/lib/libImath.so",
            "lib/libextern_bullet.a",
            "/home/jacques/blender/blender/lib/linux_x64/materialx/lib/libMaterialXFormat.so.1.39.2",
            "/home/jacques/blender/blender/lib/linux_x64/materialx/lib/libMaterialXCore.so.1.39.2",
            "lib/libbf_intern_mantaflow.a",
            "lib/libextern_mantaflow.a",
            "/home/jacques/blender/blender/lib/linux_x64/potrace/lib/libpotrace.a",
            "lib/libextern_glog.a",
            "lib/libextern_gflags.a",
            "lib/libcycles_graph.a",
            "lib/libcycles_bvh.a",
            "lib/libcycles_device.a",
            "lib/libcycles_kernel.a",
            "lib/libcycles_scene.a",
            "lib/libcycles_session.a",
            "lib/libcycles_kernel_osl.a",
            "lib/libcycles_integrator.a",
            "lib/libcycles_bvh.a",
            "lib/libcycles_device.a",
            "lib/libcycles_kernel.a",
            "lib/libcycles_scene.a",
            "lib/libcycles_session.a",
            "lib/libcycles_kernel_osl.a",
            "lib/libcycles_integrator.a",
            "/home/jacques/blender/blender/lib/linux_x64/embree/lib/libembree4.so",
            "/home/jacques/blender/blender/lib/linux_x64/embree/lib/libembree4_sycl.a",
            "/home/jacques/blender/blender/lib/linux_x64/dpcpp/lib/libsycl.so",
            "lib/libextern_cuew.a",
            "lib/libextern_hipew.a",
            "/home/jacques/blender/blender/lib/linux_x64/opencolorio/lib/libOpenColorIO.so",
            "/home/jacques/blender/blender/lib/linux_x64/alembic/lib/libAlembic.a",
            "lib/libbf_intern_sky.a",
            "/home/jacques/blender/blender/lib/linux_x64/openimageio/lib/libOpenImageIO.so",
            "/home/jacques/blender/blender/lib/linux_x64/openimageio/lib/libOpenImageIO_Util.so",
            "/home/jacques/blender/blender/lib/linux_x64/openimagedenoise/lib/libOpenImageDenoise.so",
            "/home/jacques/blender/blender/lib/linux_x64/openpgl/lib/libopenpgl.a",
            "lib/libcycles_subd.a",
            "lib/libcycles_util.a",
            "/home/jacques/blender/blender/lib/linux_x64/osl/lib/liboslcomp.so",
            "/home/jacques/blender/blender/lib/linux_x64/osl/lib/liboslexec.so",
            "/home/jacques/blender/blender/lib/linux_x64/osl/lib/liboslquery.so",
            "/home/jacques/blender/blender/lib/linux_x64/osl/lib/liboslnoise.so",
            "/home/jacques/blender/blender/lib/linux_x64/usd/lib/libusd_ms.so",
            "lib/libbf_editor_datafiles.a",
            "lib/libbf_intern_audaspace.a",
            "lib/libaudaspace-py.a",
            "lib/libaudaspace.a",
            "-lpthread",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libavformat.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libavcodec.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libavdevice.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libavutil.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libswresample.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libswscale.a",
            "/home/jacques/blender/blender/lib/linux_x64/sndfile/lib/libsndfile.a",
            "/home/jacques/blender/blender/lib/linux_x64/sndfile/lib/libFLAC.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libmp3lame.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libopus.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libtheora.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libtheoradec.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libtheoraenc.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libvorbis.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libvorbisenc.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libvorbisfile.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libogg.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libvpx.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libx264.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libx265.a",
            "/home/jacques/blender/blender/lib/linux_x64/ffmpeg/lib/libaom.a",
            "/home/jacques/blender/blender/lib/linux_x64/openal/lib/libopenal.a",
            "lib/libbf_intern_openvdb.a",
            "/home/jacques/blender/blender/lib/linux_x64/openvdb/lib/libopenvdb.so",
            "/home/jacques/blender/blender/lib/linux_x64/opencollada/lib/libOpenCOLLADAStreamWriter.a",
            "/home/jacques/blender/blender/lib/linux_x64/opencollada/lib/libOpenCOLLADASaxFrameworkLoader.a",
            "/home/jacques/blender/blender/lib/linux_x64/opencollada/lib/libOpenCOLLADAFramework.a",
            "/home/jacques/blender/blender/lib/linux_x64/opencollada/lib/libOpenCOLLADABaseUtils.a",
            "/home/jacques/blender/blender/lib/linux_x64/opencollada/lib/libGeneratedSaxParser.a",
            "/home/jacques/blender/blender/lib/linux_x64/opencollada/lib/libMathMLSolver.a",
            "/home/jacques/blender/blender/lib/linux_x64/opencollada/lib/libbuffer.a",
            "/home/jacques/blender/blender/lib/linux_x64/opencollada/lib/libftoa.a",
            "/home/jacques/blender/blender/lib/linux_x64/opencollada/lib/libUTF.a",
            "/home/jacques/blender/blender/lib/linux_x64/xml2/lib/libxml2.a",
            "lib/libextern_nanosvg.a",
            "/home/jacques/blender/blender/lib/linux_x64/pugixml/lib/libpugixml.a",
            "/home/jacques/blender/blender/lib/linux_x64/haru/lib/libhpdfs.a",
            "/home/jacques/blender/blender/lib/linux_x64/tiff/lib/libtiff.a",
            "lib/libextern_curve_fit_nd.a",
            "lib/libbf_functions.a",
            "/home/jacques/blender/blender/lib/linux_x64/epoxy/lib/libepoxy.a",
            "-Xlinker",
            "-export-dynamic",
            "/home/jacques/blender/blender/lib/linux_x64/python/lib/libpython3.11.a",
            "lib/libbf_intern_slim.a",
            "lib/libbf_blenlib.a",
            "lib/libextern_xxhash.a",
            "/home/jacques/blender/blender/lib/linux_x64/fftw3/lib/libfftw3f.a",
            "/home/jacques/blender/blender/lib/linux_x64/fftw3/lib/libfftw3.a",
            "/home/jacques/blender/blender/lib/linux_x64/fftw3/lib/libfftw3f_threads.a",
            "lib/libbf_intern_eigen.a",
            "/usr/lib/gcc/x86_64-redhat-linux/14/libgomp.a",
            "lib/libextern_wcwidth.a",
            "/home/jacques/blender/blender/lib/linux_x64/zlib/lib/libz.a",
            "/home/jacques/blender/blender/lib/linux_x64/zstd/lib/libzstd.a",
            "/home/jacques/blender/blender/lib/linux_x64/gmp/lib/libgmpxx.a",
            "/home/jacques/blender/blender/lib/linux_x64/gmp/lib/libgmp.a",
            "lib/libbf_intern_libc_compat.a",
            "lib/libbf_dna.a",
            "lib/libextern_fmtlib.a",
            "/home/jacques/blender/blender/lib/linux_x64/tbb/lib/libtbb.so",
            "lib/libbf_intern_clog.a",
            "lib/libbf_intern_guardedalloc.a",
            "-ldl",
        ];
        let raw_args: Vec<&OsStr> = raw_args.iter().map(|s| s.as_ref()).collect();
        test_round_trip(&raw_args);
    }

    fn test_round_trip(args: &[&OsStr]) {
        let parse1 = GCCArgs::parse(Path::new("/first/path"), args).expect("should be valid");
        let parse1_to_args = parse1.to_args();
        let parse2 =
            GCCArgs::parse(Path::new("/second/path"), &parse1_to_args).expect("should be valid");
        assert_eq!(parse2, parse1);
    }
}
