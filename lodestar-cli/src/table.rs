use std::io::Write;

/// Writes `head` and `rows` as left-aligned columns separated by two spaces.
pub(crate) fn write_table<const N: usize>(
    out: &mut impl Write,
    head: [&str; N],
    rows: &[[String; N]],
) -> std::io::Result<()> {
    let mut widths = head.map(str::len);
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        cells
            .iter()
            .enumerate()
            .map(|(index, cell)| format!("{cell:<width$}", width = widths[index]))
            .collect::<Vec<_>>()
            .join("  ")
    };
    writeln!(out, "{}", line(head.to_vec()).trim_end())?;
    for row in rows {
        writeln!(
            out,
            "{}",
            line(row.iter().map(String::as_str).collect()).trim_end()
        )?;
    }
    Ok(())
}
