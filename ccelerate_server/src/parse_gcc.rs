use anyhow::Result;
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use crate::path_utils::make_absolute;

#[derive(Debug, PartialEq, Eq)]
pub struct GCCArgs {
    pub sources: Vec<PathBuf>,
    pub primary_output: Option<PathBuf>,
    pub user_includes: Vec<PathBuf>,
    pub system_includes: Vec<PathBuf>,
    pub defines: Vec<String>,
    pub warnings: Vec<String>,
    pub machine_args: Vec<String>,
    pub pipe: bool,
    pub f_flags: Vec<String>,
    pub opt_flags: Vec<String>,
    pub lang_std: Option<String>,
    pub depfile_generate: bool,
    pub depfile_output_path: Option<PathBuf>,
    pub depfile_target_name: Option<String>,
    pub compile_only: bool,
    pub shared: bool,
}

impl Default for GCCArgs {
    fn default() -> Self {
        Self {
            sources: vec![],
            primary_output: None,
            user_includes: vec![],
            system_includes: vec![],
            defines: vec![],
            warnings: vec![],
            machine_args: vec![],
            pipe: false,
            f_flags: vec![],
            opt_flags: vec![],
            lang_std: None,
            depfile_generate: false,
            depfile_output_path: None,
            depfile_target_name: None,
            compile_only: false,
            shared: false,
        }
    }
}

impl GCCArgs {
    pub fn parse(cwd: &Path, raw_args: &[&OsStr]) -> Result<Self> {
        let mut args = Self::default();

        let mut raw_args_iter = raw_args.iter();
        while let Some(raw_arg) = raw_args_iter.next() {
            let arg_str = raw_arg.to_str().ok_or_else(|| {
                anyhow::anyhow!(
                    "Failed to convert OsStr to str for arg: {}",
                    raw_arg.to_string_lossy()
                )
            })?;
            if arg_str.starts_with("-D") {
                args.defines.push(arg_str[2..].to_string());
            } else if arg_str.starts_with("-I") {
                args.user_includes
                    .push(make_absolute(cwd, Path::new(&arg_str[2..])));
            } else if arg_str.starts_with("-isystem") {
                let path = raw_args_iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Missing path after -isystem"))?;
                args.system_includes
                    .push(make_absolute(cwd, Path::new(path)));
            } else if arg_str.starts_with("-W") {
                args.warnings.push(arg_str.to_string());
            } else if arg_str.starts_with("-m") {
                args.machine_args.push(arg_str.to_string());
            } else if arg_str == "-pipe" {
                args.pipe = true;
            } else if arg_str == "-shared" {
                args.shared = true;
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
                    .ok_or_else(|| anyhow::anyhow!("Missing name"))?;
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
                args.compile_only = true;
            } else {
                args.sources.push(make_absolute(cwd, Path::new(raw_arg)));
            }
        }
        Ok(args)
    }

    pub fn to_args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![];
        if self.compile_only {
            args.push("-c".into());
        }
        if self.pipe {
            args.push("-pipe".into());
        }
        if self.shared {
            args.push("-shared".into());
        }
        if let Some(lang_std) = &self.lang_std {
            args.push(lang_std.into());
        }
        for arg in &self.opt_flags {
            args.push(arg.into());
        }
        for arg in &self.f_flags {
            args.push(arg.into());
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
        if let Some(path) = &self.primary_output {
            args.push("-o".into());
            args.push(path.as_os_str().into());
        }
        for arg in &self.sources {
            args.push(arg.as_os_str().into());
        }
        args
    }
}

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
        &Path::new("/home/jacques/Documents/ccelerate_test/build_blender"),
        &raw_args,
    );
    assert!(args.is_ok());
    let args = args.unwrap();
    assert_eq!(
        args.sources,
        vec![Path::new(
            "/home/jacques/blender/blender/source/blender/blenlib/intern/BLI_mempool.cc"
        )]
    );
    assert_eq!(
        args.primary_output,
        Some(PathBuf::from(
            "/home/jacques/Documents/ccelerate_test/build_blender/source/blender/makesdna/intern/CMakeFiles/makesdna.dir/__/__/blenlib/intern/BLI_mempool.cc.o"
        ))
    );
    assert!(args.compile_only);
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
        vec![
            "/home/jacques/blender/blender/source/blender/makesdna",
            "/home/jacques/Documents/ccelerate_test/build_blender/source/blender/makesdna/intern",
            "/home/jacques/blender/blender/source/blender/blenlib",
            "/home/jacques/blender/blender/source/blender/imbuf",
            "/home/jacques/blender/blender/source/blender/imbuf/movie",
            "/home/jacques/blender/blender/intern/atomic/.",
            "/home/jacques/blender/blender/intern/guardedalloc",
            "/home/jacques/blender/blender/extern/fmtlib/include",
        ]
        .iter()
        .map(|s| PathBuf::from(s))
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

    test_round_trip(&args);
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
        &Path::new("/home/jacques/Documents/ccelerate_test/build_blender"),
        &raw_args,
    );
    assert!(args.is_ok());
    let args = args.unwrap();

    assert!(args.shared);
    assert_eq!(
        args.sources,
        vec![
            "/home/jacques/Documents/ccelerate_test/build_blender/extern/draco/CMakeFiles/extern_draco.dir/src/common.cpp.o",
            "/home/jacques/Documents/ccelerate_test/build_blender/extern/draco/CMakeFiles/extern_draco.dir/src/decoder.cpp.o",
            "/home/jacques/Documents/ccelerate_test/build_blender/extern/draco/CMakeFiles/extern_draco.dir/src/encoder.cpp.o",
            "/home/jacques/Documents/ccelerate_test/build_blender/lib/libdraco.a",
        ].iter().map(|s| Path::new(s)).collect::<Vec<_>>()
    );
    assert_eq!(args.compile_only, false);
    assert_eq!(args.depfile_generate, false);
    assert_eq!(
        args.primary_output,
        Some(PathBuf::from(
            "/home/jacques/Documents/ccelerate_test/build_blender/lib/libextern_draco.so"
        ))
    );
    assert_eq!(args.defines, vec!["NDEBUG".to_string()]);

    test_round_trip(&args);
}

fn test_round_trip(args: &GCCArgs) {
    let to_args_result = args.to_args();
    let parsed_again = GCCArgs::parse(
        &Path::new("/some/other/path"),
        to_args_result
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .as_slice(),
    );
    assert!(parsed_again.is_ok());
    let parsed_again = parsed_again.unwrap();
    assert_eq!(&parsed_again, args);
}
