//! Apply an antialiased rounded-square alpha mask to the existing artwork.
use image::{RgbaImage, imageops::FilterType};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let original = image::open("assets/renamer-icon-square-original.png")?.into_rgba8();
    let tile = image::imageops::resize(&original, 924, 924, FilterType::Lanczos3);
    let mut output = RgbaImage::new(1024, 1024);
    let radius = 208.0f64;
    for y in 0..924u32 {
        for x in 0..924u32 {
            let mut coverage = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = x as f64 + (sx as f64 + 0.5) / 4.0;
                    let py = y as f64 + (sy as f64 + 0.5) / 4.0;
                    let cx = px.clamp(radius, 924.0 - radius);
                    let cy = py.clamp(radius, 924.0 - radius);
                    if (px - cx).powi(2) + (py - cy).powi(2) <= radius * radius {
                        coverage += 1;
                    }
                }
            }
            let mut p = *tile.get_pixel(x, y);
            p[3] = ((p[3] as u32 * coverage + 8) / 16) as u8;
            output.put_pixel(x + 50, y + 50, p);
        }
    }
    println!(
        "Corner alpha: {}; center alpha: {}",
        output.get_pixel(50, 50)[3],
        output.get_pixel(512, 512)[3]
    );
    let mut preview = RgbaImage::from_pixel(1024, 1024, image::Rgba([65, 68, 76, 255]));
    image::imageops::overlay(&mut preview, &output, 0, 0);
    preview.save("/private/tmp/renamer-rounded-preview.png")?;
    output.save("assets/renamer-icon.png")?;
    Ok(())
}
