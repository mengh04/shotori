//! screencap CLI：捕获库的调试前端（与库共用 saccade::capture）

use std::fs::File;
use std::io::BufWriter;

fn main() -> anyhow::Result<()> {
    let out_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/opencode/saccade_spike2.png".into());
    let t0 = std::time::Instant::now();

    let cap = saccade::capture::capture_first_output()?;
    println!(
        "[screencap] {} {}x{} 耗时 {:?}",
        cap.output_name,
        cap.width,
        cap.height,
        t0.elapsed()
    );

    let file = File::create(&out_path)?;
    let mut enc = png::Encoder::new(BufWriter::new(file), cap.width, cap.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&cap.rgba)?;

    println!(
        "[screencap] 已保存 → {out_path}（{} KB）",
        std::fs::metadata(&out_path)?.len() / 1024
    );
    Ok(())
}
