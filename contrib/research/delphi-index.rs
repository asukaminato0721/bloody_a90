// SPDX-License-Identifier: MIT
//! Export Delphi classes, published methods, forms and event targets as TSV.
// Build this file as an example in the pinned undelphi checkout.
// This program reads a file. It does not execute the file or access USB.
use std::{
    env,
    error::Error,
    fs,
    io::{self, BufWriter, Write},
};
use undelphi::DelphiBinary;

fn cell(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn row(
    out: &mut impl Write,
    kind: &str,
    va: u64,
    owner: &str,
    name: &str,
    detail: &str,
) -> io::Result<()> {
    writeln!(
        out,
        "{kind}\t{va:08x}\t{}\t{}\t{}",
        cell(owner),
        cell(name),
        cell(detail)
    )
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = env::args().nth(1).ok_or("Supply an EXE file path")?;
    let data = fs::read(path)?;
    let bin = DelphiBinary::parse(&data)?;
    let mut out = BufWriter::new(io::stdout().lock());
    writeln!(out, "kind\tva\towner\tname\tdetail")?;
    for class in bin.classes().iter() {
        row(
            &mut out,
            "CLASS",
            class.vmt_va(),
            class.name(),
            "",
            bin.unit_name(class).unwrap_or(""),
        )?;
        for method in bin.methods(class) {
            row(
                &mut out,
                "METHOD",
                method.code_va,
                class.name(),
                method.name(),
                "",
            )?;
        }
        for method in bin.virtual_methods(class) {
            row(
                &mut out,
                "VIRTUAL",
                method.code_va,
                class.name(),
                &format!("slot_{}", method.slot),
                &format!(
                    "vmt_offset=0x{:x};slot_va=0x{:x}",
                    method.vmt_offset, method.slot_va
                ),
            )?;
        }
        for method in bin.dynamic_slots(class) {
            row(
                &mut out,
                "DYNAMIC",
                method.handler_va,
                class.name(),
                &format!("dispatch_{:04x}", method.index as u16),
                &format!("index={}", method.index),
            )?;
        }
    }
    for (resource, root) in bin.forms() {
        row(
            &mut out,
            "FORM",
            0,
            root.class_name(),
            root.object_name(),
            resource,
        )?;
        // VCL event handlers belong to the root form, including child events.
        let owner = bin.classes().find_by_name(root.class_name());
        for (path, component) in root.walk_with_path() {
            for property in &component.properties {
                if !property.name().starts_with("On") {
                    continue;
                }
                let Some(handler) = property.value.as_text_strict() else {
                    continue;
                };
                let va = owner
                    .and_then(|class| bin.resolve_event_handler(class, &handler))
                    .unwrap_or(0);
                row(
                    &mut out,
                    "EVENT",
                    va,
                    root.class_name(),
                    &format!("{path}.{}", property.name()),
                    &handler,
                )?;
            }
        }
    }
    out.flush()?;
    Ok(())
}
