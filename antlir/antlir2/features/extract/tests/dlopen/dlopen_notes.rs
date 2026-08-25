/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Tool to manage .note.dlopen (and other FDO dlopen note sections) in ELF binaries.
//!
//! Subcommands:
//!   add – encode JSON note entries and stamp them onto a binary via objcopy.
//!   get – parse an ELF file and dump a JSON list of all discovered dlopen note entries.
//!
//! The note encoding for `add` (and the decoding for `get`) follows the
//! systemd / FDO dlopen spec:
//!   n_namesz=4, n_descsz=len(json_nul), n_type=0x407c0c0a ("FDO")
//!   name = "FDO\0"
//!   desc = JSON array of entries for that section, NUL-terminated, padded to 4.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use byteorder::LittleEndian;
use byteorder::WriteBytesExt;
use clap::Parser;
use json_arg::Json;
use serde::Deserialize;
use serde::Serialize;

const NT_FDO_DLOPEN: u32 = 0x407C0C0A;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct NoteEntry {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    soname: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    feature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

type NotesDict = BTreeMap<String, Vec<NoteEntry>>;

// ---------------------------------------------------------------------------
// CLI definition with subcommands
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[clap(name = "dlopen-notes", about = "Manage .note.dlopen ELF notes")]
struct Cli {
    #[clap(subcommand)]
    command: CommandSub,
}

#[derive(Debug, clap::Subcommand)]
enum CommandSub {
    /// Add dlopen notes to an ELF file (previous add-notes behavior)
    Add(AddArgs),
    /// Dump a JSON list of all discovered dlopen note entries in an ELF file
    Get(GetArgs),
}

#[derive(Debug, Parser)]
struct AddArgs {
    #[clap(long)]
    notes: Json<NotesDict>,

    #[clap(long)]
    src: PathBuf,

    #[clap(long)]
    out: PathBuf,

    #[clap(long)]
    objcopy: PathBuf,
}

#[derive(Debug, Parser)]
struct GetArgs {
    /// Path to the ELF binary to inspect
    #[clap(value_name = "ELF")]
    elf: PathBuf,
}

// ---------------------------------------------------------------------------
// Encoding (for `add`)
// ---------------------------------------------------------------------------

fn encode_single_note(entries: &[NoteEntry]) -> Result<Vec<u8>> {
    let j = serde_json::to_string(entries).context("failed to serialize entries to JSON")?;
    let mut j_bytes = j.into_bytes();
    j_bytes.push(0);

    let mut out = Vec::with_capacity(12 + 4 + j_bytes.len() + 4);
    out.write_u32::<LittleEndian>(4).context("write namesz")?;
    out.write_u32::<LittleEndian>(j_bytes.len() as u32)
        .context("write descsz")?;
    out.write_u32::<LittleEndian>(NT_FDO_DLOPEN)
        .context("write type")?;
    out.write_all(b"FDO\0").context("write name")?;
    out.write_all(&j_bytes).context("write desc")?;
    let pad = (4 - (j_bytes.len() % 4)) % 4;
    out.write_all(&vec![0u8; pad]).context("write padding")?;

    Ok(out)
}

fn cmd_add(args: AddArgs) -> Result<()> {
    let notes: &NotesDict = &args.notes;

    if notes.is_empty() || notes.values().all(|v| v.is_empty()) {
        fs::copy(&args.src, &args.out)
            .with_context(|| format!("copy {} -> {}", args.src.display(), args.out.display()))?;
        let mut perms = fs::metadata(&args.out)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&args.out, perms)?;
        return Ok(());
    }

    let tmpdir = tempfile::tempdir().context("creating tempdir")?;
    let mut objcopy_args: Vec<String> = Vec::new();

    for (idx, (sec_name, entries)) in notes.iter().enumerate() {
        if entries.is_empty() {
            continue;
        }
        let payload = encode_single_note(entries)?;
        let note_file = tmpdir.path().join(format!("notes_{idx}.bin"));
        fs::write(&note_file, &payload)
            .with_context(|| format!("writing {}", note_file.display()))?;

        objcopy_args.push(format!(
            "--add-section={}={}",
            sec_name,
            note_file.display()
        ));
        objcopy_args.push("--set-section-flags".to_string());
        objcopy_args.push(format!("{sec_name}=alloc,readonly"));
    }

    let mut cmd = Command::new(&args.objcopy);
    cmd.args(&objcopy_args).arg(&args.src).arg(&args.out);

    let status = cmd
        .status()
        .with_context(|| format!("failed to execute objcopy {}", args.objcopy.display()))?;
    if !status.success() {
        anyhow::bail!("objcopy failed with status {status}: command was {cmd:?}");
    }

    let mut perms = fs::metadata(&args.out)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&args.out, perms)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Decoding (for `get`)
// ---------------------------------------------------------------------------

fn dlopen_entries_from_desc(desc: &[u8]) -> Vec<NoteEntry> {
    let s = String::from_utf8_lossy(desc);
    let trimmed = s.trim_matches(|c: char| c == '\0').trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<Vec<NoteEntry>>(trimmed) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("warning: failed to parse .note.dlopen JSON payload: {e}");
            Vec::new()
        }
    }
}

fn cmd_get(args: GetArgs) -> Result<()> {
    let data =
        fs::read(&args.elf).with_context(|| format!("failed to read {}", args.elf.display()))?;

    let elf = goblin::elf::Elf::parse(&data)
        .with_context(|| format!("failed to parse ELF {}", args.elf.display()))?;

    let mut all_entries: Vec<NoteEntry> = Vec::new();

    let mut iters = Vec::new();
    if let Some(it) = elf.iter_note_headers(&data) {
        iters.push(it);
    }
    if let Some(it) = elf.iter_note_sections(&data, None) {
        iters.push(it);
    }

    for iter in iters {
        for note_res in iter {
            if let Ok(note) = note_res {
                if note.n_type == NT_FDO_DLOPEN && note.name == "FDO" {
                    all_entries.extend(dlopen_entries_from_desc(note.desc));
                }
            }
        }
    }

    // Output JSON list to stdout
    serde_json::to_writer_pretty(std::io::stdout(), &all_entries)
        .context("failed to write JSON")?;
    println!();

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        CommandSub::Add(args) => cmd_add(args),
        CommandSub::Get(args) => cmd_get(args),
    }
}
