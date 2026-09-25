//! screencap CLI: a debugging front end for the capture library
//! (shares shotori::capture with the library).
//! Usage: screencap [output path prefix] [--all]
//!   Saves the first output by default; with --all, one PNG per output as
//!   <prefix>_<output-name>.png

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let all = args.iter().any(|a| a == "--all");
    let prefix = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(|s| s.trim_end_matches(".png").to_string())
        .unwrap_or_else(|| "/tmp/screencap".into());
    let t0 = std::time::Instant::now();

    let caps = shotori::capture::capture_all_outputs()?;
    println!(
        "[screencap] {} output(s) in {:?}",
        caps.len(),
        t0.elapsed()
    );

    let save = |cap: &shotori::capture::Capture, path: String| -> anyhow::Result<()> {
        let mut f = std::io::BufWriter::new(std::fs::File::create(&path)?);
        let mut enc = png::Encoder::new(&mut f, cap.width, cap.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&cap.rgba)?;
        println!(
            "[screencap] {} {}x{}{} → {} ({} KB)",
            cap.output_name,
            cap.width,
            cap.height,
            if cap.rotated() { " (rotated per transform)" } else { "" },
            path,
            cap.rgba.len() / 1024
        );
        Ok(())
    };

    if all {
        for cap in &caps {
            save(cap, format!("{prefix}_{}.png", cap.output_name))?;
        }
    } else {
        let cap = caps.first().expect("at least one output");
        save(cap, format!("{prefix}.png"))?;
    }
    Ok(())
}