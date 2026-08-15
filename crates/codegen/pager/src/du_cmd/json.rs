//! JSON rendering for `grow du`.

use serde::Serialize;

use super::{DuReport, SCHEMA_VERSION};

pub(super) fn write(report: &DuReport, writer: &mut impl std::io::Write) -> anyhow::Result<()> {
    serde_json::to_writer_pretty(&mut *writer, &JsonReport::from(report))?;
    writeln!(writer)?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonReport<'a> {
    schema_version: &'static str,
    root: String,
    total_bytes: u64,
    entries: Vec<JsonEntry<'a>>,
    warnings: Vec<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonEntry<'a> {
    name: &'a str,
    bytes: u64,
}

impl<'a> From<&'a DuReport> for JsonReport<'a> {
    fn from(report: &'a DuReport) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            root: report.root.display().to_string(),
            total_bytes: report.total_bytes,
            entries: report
                .entries
                .iter()
                .map(|entry| JsonEntry {
                    name: &entry.name,
                    bytes: entry.bytes,
                })
                .collect(),
            warnings: report.warnings.iter().map(String::as_str).collect(),
        }
    }
}
