use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use anyhow::Result;

use crate::path_utils::make_absolute;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ArArgs {
    pub flag_q: bool,
    pub flag_c: bool,
    pub flag_s: bool,
    pub flag_T: bool,
    pub output: Option<PathBuf>,
    pub sources: Vec<PathBuf>,
}

impl Default for ArArgs {
    fn default() -> Self {
        Self {
            flag_q: false,
            flag_c: false,
            flag_s: false,
            flag_T: false,
            output: None,
            sources: vec![],
        }
    }
}

impl ArArgs {
    pub fn parse(cwd: &Path, raw_args: &[&OsStr]) -> Result<Self> {
        let mut args = Self::default();
        let mut raw_args_iter = raw_args.iter();
        let first_arg = raw_args_iter
            .next()
            .ok_or_else(|| anyhow::anyhow!("Missing first argument for ar command"))?;
        for c in first_arg.to_string_lossy().chars() {
            match c {
                'q' => args.flag_q = true,
                'c' => args.flag_c = true,
                's' => args.flag_s = true,
                'T' => args.flag_T = true,
                _ => return Err(anyhow::anyhow!("Unknown ar flag: {}", c)),
            }
        }
        while let Some(raw_arg) = raw_args_iter.next() {
            let abs_path = make_absolute(cwd, Path::new(raw_arg));
            if args.output.is_none() {
                args.output = Some(abs_path);
            } else {
                args.sources.push(abs_path);
            }
        }
        Ok(args)
    }

    pub fn to_args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![];
        let mut first_arg = OsString::from("");
        if self.flag_q {
            first_arg.push("q");
        }
        if self.flag_c {
            first_arg.push("c");
        }
        if self.flag_s {
            first_arg.push("s");
        }
        if self.flag_T {
            first_arg.push("T");
        }
        args.push(first_arg);
        if let Some(output) = &self.output {
            args.push(output.as_os_str().into());
        }
        for arg in &self.sources {
            args.push(arg.as_os_str().into());
        }
        args
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_ar() {
        let raw_args = vec![
            "qc",
            "lib/libbf_io_grease_pencil.a",
            "source/blender/io/grease_pencil/CMakeFiles/bf_io_grease_pencil.dir/intern/grease_pencil_io.cc.o",
            "source/blender/io/grease_pencil/CMakeFiles/bf_io_grease_pencil.dir/intern/grease_pencil_io_import_svg.cc.o",
            "source/blender/io/grease_pencil/CMakeFiles/bf_io_grease_pencil.dir/intern/grease_pencil_io_export_svg.cc.o",
            "source/blender/io/grease_pencil/CMakeFiles/bf_io_grease_pencil.dir/intern/grease_pencil_io_export_pdf.cc.o",
        ];
        let raw_args: Vec<&OsStr> = raw_args.iter().map(|s| s.as_ref()).collect();

        let args = ArArgs::parse(
            &Path::new("/home/jacques/Documents/ccelerate_test/build_blender"),
            &raw_args,
        );
        assert!(args.is_ok());
        let args = args.unwrap();
        assert_eq!(args.flag_q, true);
        assert_eq!(args.flag_c, true);
        assert_eq!(
            args.output,
            Some(PathBuf::from(
                "/home/jacques/Documents/ccelerate_test/build_blender/lib/libbf_io_grease_pencil.a"
            ))
        );
        assert_eq!(
            args.sources,
            vec![
                "/home/jacques/Documents/ccelerate_test/build_blender/source/blender/io/grease_pencil/CMakeFiles/bf_io_grease_pencil.dir/intern/grease_pencil_io.cc.o",
                "/home/jacques/Documents/ccelerate_test/build_blender/source/blender/io/grease_pencil/CMakeFiles/bf_io_grease_pencil.dir/intern/grease_pencil_io_import_svg.cc.o",
                "/home/jacques/Documents/ccelerate_test/build_blender/source/blender/io/grease_pencil/CMakeFiles/bf_io_grease_pencil.dir/intern/grease_pencil_io_export_svg.cc.o",
                "/home/jacques/Documents/ccelerate_test/build_blender/source/blender/io/grease_pencil/CMakeFiles/bf_io_grease_pencil.dir/intern/grease_pencil_io_export_pdf.cc.o",
            ].iter().map(|s| Path::new(s)).collect::<Vec<_>>()
        );

        test_round_trip(&raw_args);
    }

    fn test_round_trip(args: &[&OsStr]) {
        let parse1 = ArArgs::parse(
            &Path::new("/first/path"),
            args.iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .as_slice(),
        );
        let parse1_to_args = parse1.as_ref().unwrap().to_args();
        let parse2 = ArArgs::parse(
            &Path::new("/second/path"),
            parse1_to_args
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .as_slice(),
        );
        assert_eq!(parse2.unwrap(), parse1.unwrap());
    }
}
