use std::{
    error::Error,
    ffi::OsStr,
    io::{self, Write},
    process::ExitCode,
};

use crate::cli::{CacheArgs, CacheCommand, CacheProjectArgs};

pub(crate) fn run(args: CacheArgs) -> Result<ExitCode, Box<dyn Error>> {
    match args.command {
        CacheCommand::Refresh(args) => refresh(args),
    }
}

pub(crate) fn refresh(args: CacheProjectArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let engine = crate::engine::resolve(&project, args.godot.as_deref())?;
    let (version, _) = crate::engine::validated_version(&engine, &project)?;
    eprintln!(
        "engine: {} ({version})",
        crate::engine::display_path(&engine)
    );
    crate::import_worker::stop_project(&project)?;
    let result = crate::check::engine_output(
        &engine,
        &project,
        &[OsStr::new("--import"), OsStr::new("--quiet")],
    )?;
    io::stderr().write_all(&result.output.stdout)?;
    io::stderr().write_all(&result.output.stderr)?;
    let failed = !result.output.status.success() || crate::check::has_errors(&result.output);
    println!("cache refresh {}", if failed { "failed" } else { "passed" });
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
