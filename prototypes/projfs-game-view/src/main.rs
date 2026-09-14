mod resolver;

#[cfg(windows)]
mod windows_provider;

use std::env;
use std::path::PathBuf;

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug)]
struct ServeArgs {
    base: PathBuf,
    view: PathBuf,
    overwrite: PathBuf,
    state: PathBuf,
    mods: Vec<PathBuf>,
    ready_file: PathBuf,
    stop_file: PathBuf,
}

fn usage() -> &'static str {
    "usage:\n  projfs-game-view serve --base PATH --view PATH --overwrite PATH --state PATH [--mod PATH ...] --ready-file PATH --stop-file PATH\n  projfs-game-view probe --expect RELATIVE_PATH=VALUE [--expect ...]"
}

fn take_value(args: &[String], index: &mut usize, option: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("missing value for {option}"))
}

fn parse_serve(args: &[String]) -> Result<ServeArgs, String> {
    let mut base = None;
    let mut view = None;
    let mut overwrite = None;
    let mut state = None;
    let mut mods = Vec::new();
    let mut ready_file = None;
    let mut stop_file = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--base" => base = Some(take_value(args, &mut index, "--base")?.into()),
            "--view" => view = Some(take_value(args, &mut index, "--view")?.into()),
            "--overwrite" => overwrite = Some(take_value(args, &mut index, "--overwrite")?.into()),
            "--state" => state = Some(take_value(args, &mut index, "--state")?.into()),
            "--mod" => mods.push(take_value(args, &mut index, "--mod")?.into()),
            "--ready-file" => {
                ready_file = Some(take_value(args, &mut index, "--ready-file")?.into())
            }
            "--stop-file" => stop_file = Some(take_value(args, &mut index, "--stop-file")?.into()),
            unknown => return Err(format!("unknown serve option: {unknown}")),
        }
        index += 1;
    }
    Ok(ServeArgs {
        base: base.ok_or("missing --base")?,
        view: view.ok_or("missing --view")?,
        overwrite: overwrite.ok_or("missing --overwrite")?,
        state: state.ok_or("missing --state")?,
        mods,
        ready_file: ready_file.ok_or("missing --ready-file")?,
        stop_file: stop_file.ok_or("missing --stop-file")?,
    })
}

fn probe(args: &[String]) -> Result<(), String> {
    let cwd = env::current_dir().map_err(|error| error.to_string())?;
    println!("probe cwd={}", cwd.display());
    let mut expectations = 0;
    let mut index = 0;
    while index < args.len() {
        if args[index] != "--expect" {
            return Err(format!("unknown probe option: {}", args[index]));
        }
        let value = take_value(args, &mut index, "--expect")?;
        let (path, expected) = value
            .split_once('=')
            .ok_or_else(|| format!("expected PATH=VALUE, got {value}"))?;
        let actual = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {path}: {error}"))?;
        if actual != expected {
            return Err(format!("{path}: expected {expected:?}, got {actual:?}"));
        }
        println!("probe read {path}={actual:?}");
        expectations += 1;
        index += 1;
    }
    if expectations == 0 {
        return Err("probe requires at least one --expect PATH=VALUE".into());
    }
    println!("PROBE PASS ({expectations} files)");
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        return Err(usage().into());
    };
    match command {
        "probe" => probe(&args[1..]),
        "serve" => {
            let serve = parse_serve(&args[1..])?;
            #[cfg(windows)]
            {
                windows_provider::serve(serve)
            }
            #[cfg(not(windows))]
            {
                let _ = serve;
                Err(
                    "serve requires Windows 11 with the Projected File System feature enabled"
                        .into(),
                )
            }
        }
        _ => Err(usage().into()),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
