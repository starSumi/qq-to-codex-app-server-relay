use std::{env, fs, path::PathBuf, process::ExitCode};

fn run() -> anyhow::Result<()> {
    let mut args = env::args().skip(1);
    let mode = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: export-schema (--out|--check) <path>"))?;
    let path = PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("schema output path is required"))?,
    );
    if args.next().is_some() {
        anyhow::bail!("unexpected extra arguments");
    }

    let expected = relay_protocol::schema_json()?;
    match mode.as_str() {
        "--out" => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, expected)?;
        }
        "--check" => {
            let actual = fs::read_to_string(&path)?;
            if actual != expected {
                anyhow::bail!("{} is stale; run `pnpm schema:generate`", path.display());
            }
        }
        _ => anyhow::bail!("unknown mode {mode}; expected --out or --check"),
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("export-schema: {error:#}");
            ExitCode::FAILURE
        }
    }
}
