//! Summarizes recorded measurements, observations, conditions, medications and ECGs.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, ReadDir};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Maximum report size within the Ark's 1 MiB standard output limit.
const MAX_REPORT_BYTES: usize = 1024 * 1024;
/// Maximum entries shown per list, with the remainder counted.
const MAX_ROWS: usize = 20;
/// Required kinds in report order, with names for absent views.
const KINDS: [(&str, &str); 4] = [
    ("loinc-8867-4", "Heart rate"),
    ("loinc-29463-7", "Body weight"),
    ("loinc-8480-6", "Systolic blood pressure"),
    ("loinc-8462-4", "Diastolic blood pressure"),
];

/// Prints the manifest or a complete report, returning failure on read errors.
fn main() {
    let mut args = std::env::args_os().skip(1);
    let Some(root) = args.next() else {
        print!(
            r#"manifest = 1

[app]
name = "Health Summary"
version = "0.2.0"

[reads]
paths = [
    "v1/health/kinds/loinc-8867-4",
    "v1/health/kinds/loinc-29463-7",
    "v1/health/kinds/loinc-8480-6",
    "v1/health/kinds/loinc-8462-4",
    "v1/health/clinical/Observation",
    "v1/health/conditions/active",
]
optional = [
    "v1/health/medications/current",
    "v1/health/signals",
]

[listing]
language = "en"
icon = "📋"
summary = "Summarizes recorded measurements, conditions, medications and ECG lengths."
category = "tools"
license = "BSD-3-Clause"
source = "https://github.com/dark-bio/examples/tree/main/apps/13-health-summary"
keywords = ["health records", "measurements", "observations", "ECG"]
description = '''
Reads heart rate, body weight and blood pressure records. Shows their counts, first and last recorded times, numeric ranges by unit and latest values.

Counts observations by category and lists recorded active conditions. Current medications and ECG lengths appear when their optional data is available.

Summarizes what the records contain. It does not diagnose, reconcile medications, convert units or interpret ECG waveforms.
'''

[listing.purposes]
"v1/health/kinds/loinc-8867-4" = "Count heart rate records and summarize their values and times"
"v1/health/kinds/loinc-29463-7" = "Count body weight records and summarize their values and times"
"v1/health/kinds/loinc-8480-6" = "Count systolic blood pressure records and summarize their values and times"
"v1/health/kinds/loinc-8462-4" = "Count diastolic blood pressure records and summarize their values and times"
"v1/health/clinical/Observation" = "Count recorded observations by category"
"v1/health/conditions/active" = "List the conditions recorded as active"
"v1/health/medications/current" = "Optionally list the medications recorded as current"
"v1/health/signals" = "Optionally summarize ECG lengths, channel counts and sample counts"
"#
        );
        return;
    };
    if args.next().is_some() {
        eprintln!("health-summary accepts one data root");
        std::process::exit(2);
    }

    // Withhold partial reports when a granted view cannot be read
    match report(Path::new(&root)) {
        Ok(report) => print!("{report}"),
        Err(error) => {
            eprintln!("health-summary failed: {error}");
            std::process::exit(1);
        }
    }
}

/// Builds all sections in stable order using only the declared view paths.
fn report(root: &Path) -> io::Result<String> {
    let health = root.join("v1/health");
    let mut evidence = String::from("## Evidence\n\n");
    let (entries, populated) = kinds(&health.join("kinds"), &mut evidence)?;
    observations(&health.join("clinical/Observation/entries"), &mut evidence)?;
    answers(
        &health.join("conditions/active"),
        "Recorded active conditions",
        false,
        &mut evidence,
    )?;
    answers(
        &health.join("medications/current"),
        "Recorded current medications",
        true,
        &mut evidence,
    )?;
    signals(&health.join("signals"), &mut evidence)?;

    // Put the finding before its evidence, including a zero-entry finding
    let mut output = format!(
        "# Health Record Summary\n\nThe records contain {entries} measurement entries across {populated} of the 4 requested kinds.\n\n"
    );
    append(&mut output, &evidence)?;
    append(
        &mut output,
        "## Method\n\nEach measurement count includes every entry for that code. First and last are the nonempty recorded times in the supplied order. Numeric minima and maxima are grouped by the exact unit; the latest value is read from the Ark.\n\nEach observation counts once per distinct category coding, or as uncategorized when none is recorded. Condition and medication names are read from the supplied lists and sorted. ECG length is the number of sample rows multiplied by the period in milliseconds, divided by 1000 to give seconds.\n\n## Limitations\n\nThese records do not establish a diagnosis or whether a listed medication is being taken. Measurements combine sources without unit conversion or checks against reference ranges.\n\nECG length includes rows with missing or out-of-range samples. Waveforms are not interpreted. Missing data is not evidence that a measurement, condition, medication or recording never existed. An unavailable optional view can mean access was declined or the data is absent.\n",
    )?;
    Ok(output)
}

/// Creates a malformed-view error without quoting health values.
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Adds the view path to an error while preserving its kind.
fn at(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

/// Appends a fragment within the report's memory and stdout budget.
fn append(output: &mut String, fragment: &str) -> io::Result<()> {
    if output.len() + fragment.len() > MAX_REPORT_BYTES {
        return Err(invalid("report exceeds 1 MiB"));
    }
    output.push_str(fragment);
    Ok(())
}

/// Writes escaped cells in a table whose columns also align in plain text.
fn table<const N: usize>(
    output: &mut String,
    headings: [&str; N],
    rows: &[[String; N]],
) -> io::Result<()> {
    let headings = headings.map(str::to_owned);
    let widths: [usize; N] = std::array::from_fn(|column| {
        rows.iter()
            .map(|row| row[column].chars().count())
            .chain(std::iter::once(headings[column].chars().count()))
            .max()
            .unwrap()
            .max(3)
    });
    let separators = widths.map(|width| "-".repeat(width));
    for row in [&headings, &separators].into_iter().chain(rows) {
        append(output, "|")?;
        for (value, width) in row.iter().zip(widths) {
            append(output, &format!(" {value:<width$} |"))?;
        }
        append(output, "\n")?;
    }
    append(output, "\n")
}

/// Escapes view text for plain Markdown and suppresses control characters.
fn cell(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        match ch {
            '&' | '<' | '>' | '|' | '\\' | '`' | '*' | '_' | '[' | ']' | '#' | '!' => {
                output.push('\\');
                output.push(ch);
            }
            ch if ch.is_control()
                || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}') =>
            {
                output.push(' ');
            }
            _ => output.push(ch),
        }
    }
    output
}

/// Reads a UTF-8 document, retaining the filesystem error kind.
fn text(path: &Path) -> io::Result<String> {
    fs::read_to_string(path).map_err(|error| at(path, error))
}

/// Reads one UTF-8 line without stripping empty TSV fields.
fn read_line(reader: &mut impl BufRead, line: &mut String) -> io::Result<bool> {
    line.clear();
    let size = reader.read_line(line)?;
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(size != 0)
}

/// Visits nonempty lines, attaching line numbers to parse failures.
fn scan(
    reader: &mut impl BufRead,
    mut visit: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<()> {
    let mut line = String::new();
    let mut count = 0;
    while read_line(reader, &mut line)? {
        count += 1;
        if !line.trim().is_empty() {
            visit(&line).map_err(|error| invalid(format!("line {count}: {error}")))?;
        }
    }
    Ok(())
}

/// Visits a file's lines without holding the entire view in memory.
fn lines(path: &Path, visit: impl FnMut(&str) -> io::Result<()>) -> io::Result<()> {
    let file = File::open(path).map_err(|error| at(path, error))?;
    scan(&mut BufReader::new(file), visit).map_err(|error| at(path, error))
}

/// Lists child directories in lexical order, rejecting symlinks.
fn children(entries: ReadDir) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(invalid("view directory contains a symlink"));
        }
        if kind.is_dir() {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

/// Counts kind entries with numeric extrema grouped by their exact unit.
#[derive(Default)]
struct Summary {
    /// Number of entries, including text and coded values.
    count: u64,
    /// First nonempty timestamp in view order.
    first: String,
    /// Last nonempty timestamp in view order.
    last: String,
    /// Numeric extrema per unit, without conversion or source selection.
    extrema: BTreeMap<String, (f64, f64)>,
}

/// Reads required TSV columns by name, tolerating reordered and extra columns.
fn summarize(reader: &mut impl BufRead) -> io::Result<Summary> {
    let mut summary = Summary::default();
    let mut columns = None;
    scan(reader, |line| {
        let fields: Vec<_> = line.split('\t').collect();
        let Some(indices) = &columns else {
            let mut indices = Vec::new();
            for name in ["time", "value", "unit", "source"] {
                if fields.iter().filter(|field| **field == name).count() != 1 {
                    return Err(invalid(format!("expected one {name} column")));
                }
                indices.push(fields.iter().position(|field| *field == name).unwrap());
            }
            columns = Some(indices);
            return Ok(());
        };

        // Retain fields needed by the summary, leaving sources anonymous
        let row = indices
            .iter()
            .map(|index| {
                fields
                    .get(*index)
                    .copied()
                    .ok_or_else(|| invalid("short TSV row"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let time = row[0];
        let unit = row[2];
        summary.count += 1;
        if !time.is_empty() {
            if summary.first.is_empty() {
                summary.first = time.into();
            }
            summary.last = time.into();
        }
        if let Ok(value) = row[1].parse::<f64>() {
            if !value.is_finite() {
                return Err(invalid("numeric value is not finite"));
            }
            let range = summary.extrema.entry(unit.into()).or_insert((value, value));
            range.0 = range.0.min(value);
            range.1 = range.1.max(value);
        }
        Ok(())
    })?;
    if columns.is_none() {
        return Err(invalid("entries has no TSV header"));
    }
    Ok(summary)
}

/// Formats a value with its unit, explicitly marking an absent unit.
fn quantity(value: &str, unit: &str) -> String {
    format!(
        "{} {}",
        cell(value),
        cell(if unit.is_empty() {
            "(unit unspecified)"
        } else {
            unit
        })
    )
}

/// Formats numeric ranges in unit order, counting any omitted units.
fn extrema(summary: &Summary, maximum: bool) -> String {
    if summary.extrema.is_empty() {
        return "No numeric data".into();
    }
    let mut values = summary
        .extrema
        .iter()
        .take(MAX_ROWS)
        .map(|(unit, (min, max))| quantity(&if maximum { *max } else { *min }.to_string(), unit))
        .collect::<Vec<_>>();
    if summary.extrema.len() > MAX_ROWS {
        values.push(format!(
            "{} more units omitted",
            summary.extrema.len() - MAX_ROWS
        ));
    }
    values.join("; ")
}

/// Reports required kinds directly and returns entry and populated-kind counts.
fn kinds(path: &Path, output: &mut String) -> io::Result<(u64, usize)> {
    let mut total = 0;
    let mut populated = 0;
    for (code, fallback) in KINDS {
        let kind = path.join(code);
        let entries = kind.join("entries");
        let summary = match File::open(&entries) {
            Ok(file) => {
                summarize(&mut BufReader::new(file)).map_err(|error| at(&entries, error))?
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Summary::default(),
            Err(error) => return Err(at(&entries, error)),
        };
        let loinc = code.strip_prefix("loinc-").unwrap();
        if summary.count == 0 {
            append(
                output,
                &format!("### {fallback}\n\nLOINC {loinc} has 0 recorded entries. No data.\n\n"),
            )?;
            continue;
        }

        // Use the supplied name and latest value without recomputing that answer
        total += summary.count;
        populated += 1;
        let name = text(&kind.join("name"))?;
        let latest = text(&kind.join("latest/value"))?;
        let unit = match text(&kind.join("latest/unit")) {
            Ok(unit) => unit,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        append(
            output,
            &format!(
                "### {}\n\nLOINC {loinc} has {} recorded entries.\n\n",
                cell(name.trim_end_matches('\n')),
                summary.count
            ),
        )?;
        let rows = [
            (
                "First time",
                cell(if summary.first.is_empty() {
                    "Unknown"
                } else {
                    &summary.first
                }),
            ),
            (
                "Last time",
                cell(if summary.last.is_empty() {
                    "Unknown"
                } else {
                    &summary.last
                }),
            ),
            ("Minimum", extrema(&summary, false)),
            ("Maximum", extrema(&summary, true)),
            (
                "Latest",
                quantity(latest.trim_end_matches('\n'), unit.trim_end_matches('\n')),
            ),
        ]
        .map(|(label, value)| [label.to_owned(), value]);
        table(output, ["Measure", "Recorded value"], &rows)?;
    }
    Ok((total, populated))
}

/// Counts each observation once per category coding, retaining uncategorized rows.
fn observations(path: &Path, output: &mut String) -> io::Result<()> {
    let mut categories = BTreeMap::<String, u64>::new();
    let result = lines(path, |line| {
        let entry: Value =
            serde_json::from_str(line).map_err(|_| invalid("invalid Observation JSON"))?;
        if entry["resourceType"] != "Observation" {
            return Err(invalid("clinical entry is not an Observation"));
        }
        let mut distinct = BTreeSet::new();
        if let Some(values) = entry["category"].as_array() {
            for category in values {
                if let Some(codings) = category["coding"].as_array() {
                    for coding in codings {
                        if let Some(code) = coding["code"].as_str() {
                            let system = coding["system"].as_str().unwrap_or("unspecified");
                            let name = if system
                                == "http://terminology.hl7.org/CodeSystem/observation-category"
                            {
                                code.to_owned()
                            } else {
                                format!("{system} {code}")
                            };
                            distinct.insert(name);
                        }
                    }
                }
            }
        }
        if distinct.is_empty() {
            distinct.insert("Uncategorized".into());
        }
        for name in distinct {
            *categories.entry(name).or_default() += 1;
        }
        Ok(())
    });
    if let Err(error) = result
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error);
    }

    // A record with several categories contributes once to each category
    append(
        output,
        "### Observations by category\n\nAn observation can appear in several categories.\n\n",
    )?;
    let mut rows = Vec::new();
    if categories.is_empty() {
        rows.push(["No data".into(), "0".into()]);
    }
    for (name, count) in categories.iter().take(MAX_ROWS) {
        rows.push([cell(name), count.to_string()]);
    }
    table(output, ["Category", "Observations"], &rows)?;
    omitted(output, categories.len(), "categories")
}

/// Counts entries beyond a displayed list's row limit.
fn omitted(output: &mut String, count: usize, noun: &str) -> io::Result<()> {
    if count > MAX_ROWS {
        append(
            output,
            &format!("{} more {noun} omitted.\n\n", count - MAX_ROWS),
        )?;
    }
    Ok(())
}

/// Lists supplied answer names and kinds, or reports unavailable data.
fn answers(path: &Path, title: &str, optional: bool, output: &mut String) -> io::Result<()> {
    append(output, &format!("### {title}\n\n"))?;
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if optional && unavailable(&error) => {
            append(
                output,
                "Not available (access declined or data absent).\n\n",
            )?;
            return Ok(());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            append(output, "No data.\n\n")?;
            return Ok(());
        }
        Err(error) => return Err(at(path, error)),
    };
    let mut names = Vec::new();
    for child in children(entries).map_err(|error| at(path, error))? {
        let kind = child.file_name().unwrap().to_string_lossy().into_owned();
        names.push((
            text(&child.join("name"))?.trim_end_matches('\n').to_owned(),
            kind,
        ));
    }
    names.sort();
    if names.is_empty() {
        append(output, "No data.\n\n")?;
    } else {
        let rows: Vec<_> = names
            .iter()
            .take(MAX_ROWS)
            .map(|(name, kind)| [cell(name), format!("`{kind}`")])
            .collect();
        table(output, ["Recorded name", "Kind"], &rows)?;
        omitted(output, names.len(), "names")?;
    }
    Ok(())
}

/// Recognizes an omitted optional mount, including WASI capability denial.
fn unavailable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
    )
}

/// Reads a positive finite number from a signal's metadata.
fn positive(value: Option<&Value>, field: &str) -> io::Result<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| invalid(format!("signal has invalid {field}")))
}

/// Reports ECG durations and channels after checking every physical sample row.
fn signals(path: &Path, output: &mut String) -> io::Result<()> {
    append(output, "### ECG recordings\n\n")?;
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if unavailable(&error) => {
            append(
                output,
                "Not available (access declined or data absent).\n\n",
            )?;
            return Ok(());
        }
        Err(error) => return Err(at(path, error)),
    };
    let recordings = children(entries).map_err(|error| at(path, error))?;
    if recordings.is_empty() {
        append(output, "No data.\n\n")?;
        return Ok(());
    }
    for (index, recording) in recordings.iter().enumerate() {
        let info_path = recording.join("info");
        let info: Value = serde_json::from_str(&text(&info_path)?)
            .map_err(|_| at(&info_path, invalid("invalid signal JSON")))?;
        let channels = info["channels"]
            .as_u64()
            .filter(|count| *count > 0)
            .ok_or_else(|| at(&info_path, invalid("signal has invalid channels")))?;
        let period = positive(
            info.get("period_ms").or_else(|| info.get("period")),
            "period",
        )
        .map_err(|error| at(&info_path, error))?;

        // Each row is one time point regardless of the number of channels
        let mut samples = 0_u64;
        lines(&recording.join("samples"), |line| {
            let mut count = 0;
            for value in line.split('\t') {
                if !matches!(value, "E" | "L" | "U")
                    && !value.parse::<f64>().is_ok_and(|number| number.is_finite())
                {
                    return Err(invalid("signal sample is not a finite number or E, L, U"));
                }
                count += 1;
            }
            if count != channels {
                return Err(invalid("signal sample row has a different channel count"));
            }
            samples += 1;
            Ok(())
        })?;
        if let Some(count) = info.get("sample_count")
            && count.as_u64() != Some(samples)
        {
            return Err(at(
                &info_path,
                invalid("signal sample count differs from info"),
            ));
        }
        let duration = period * samples as f64 / 1000.0;
        if !duration.is_finite() {
            return Err(at(&info_path, invalid("signal duration is not finite")));
        }
        if index < MAX_ROWS {
            append(
                output,
                &format!(
                    "Recording {} started at {}.\n\n",
                    cell(&recording.file_name().unwrap().to_string_lossy()),
                    cell(info["start"].as_str().unwrap_or("an unknown time")),
                ),
            )?;
            table(
                output,
                ["Length", "Channels", "Sample rows"],
                &[[
                    format!("{duration}s"),
                    channels.to_string(),
                    samples.to_string(),
                ]],
            )?;
        }
    }
    omitted(output, recordings.len(), "recordings")
}

/// Parsing checks using invented records independent of the fixture layout.
#[cfg(test)]
mod tests {
    use super::*;

    /// Keeps units separate and accepts extra columns without assuming their order.
    #[test]
    fn test_mixed_units_and_reordered_columns() {
        let input = "source\tunit\ttime\tvalue\textra\na\tkg\t2025-01-01T00:00:00Z\t70.5\tx\nb\t[lb_av]\t2025-01-02T00:00:00Z\t154\tx\na\tkg\t2025-01-03T00:00:00Z\t70\tx\n";
        let summary = summarize(&mut input.as_bytes()).unwrap();
        assert_eq!(summary.count, 3);
        assert_eq!(summary.first, "2025-01-01T00:00:00Z");
        assert_eq!(summary.last, "2025-01-03T00:00:00Z");
        assert_eq!(summary.extrema.get("kg"), Some(&(70.0, 70.5)));
        assert_eq!(summary.extrema.get("[lb_av]"), Some(&(154.0, 154.0)));
    }

    /// Counts text results and missing times without inventing numeric extrema.
    #[test]
    fn test_text_results_and_missing_times() {
        let summary =
            summarize(&mut "time\tvalue\tunit\tsource\n\tNo growth\t\ta\n".as_bytes()).unwrap();
        assert_eq!(summary.count, 1);
        assert_eq!(summary.first, "");
        assert_eq!(summary.last, "");
        assert!(summary.extrema.is_empty());
    }

    /// Distinguishes missing headers, short rows and nonfinite numeric readings.
    #[test]
    fn test_invalid_entries_report_specific_errors() {
        for (input, expected) in [
            ("", "entries has no TSV header"),
            ("value\tunit\tsource\n", "line 1: expected one time column"),
            (
                "time\tvalue\tunit\tsource\ntime\t1\tkg\n",
                "line 2: short TSV row",
            ),
            (
                "time\tvalue\tunit\tsource\ntime\tNaN\tkg\ta\n",
                "line 2: numeric value is not finite",
            ),
        ] {
            let error = summarize(&mut input.as_bytes()).err().unwrap();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{input}");
            assert_eq!(error.to_string(), expected, "{input}");
        }
    }
}
