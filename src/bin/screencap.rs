//! screencap CLI：捕获库的调试前端（与库共用 saccade::capture）
//! 用法：screencap [输出路径前缀] [--all]
//!   默认存第一块屏；--all 时每屏一张 <前缀>_<输出名>.png

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let all = args.iter().any(|a| a == "--all");
    let prefix = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(|s| s.trim_end_matches(".png").to_string())
        .unwrap_or_else(|| "/tmp/screencap".into());
    let t0 = std::time::Instant::now();

    let caps = saccade::capture::capture_all_outputs()?;
    println!(
        "[screencap] {} 块屏，耗时 {:?}",
        caps.len(),
        t0.elapsed()
    );

    let save = |cap: &saccade::capture::Capture, path: String| -> anyhow::Result<()> {
        let mut f = std::io::BufWriter::new(std::fs::File::create(&path)?);
        let mut enc = png::Encoder::new(&mut f, cap.width, cap.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&cap.rgba)?;
        println!(
            "[screencap] {} {}x{}{} → {}（{} KB）",
            cap.output_name,
            cap.width,
            cap.height,
            if cap.rotated() { "（已按 transform 旋转）" } else { "" },
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
        let cap = caps.first().expect("至少一块屏");
        save(cap, format!("{prefix}.png"))?;
    }
    Ok(())
}
