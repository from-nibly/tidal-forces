//! Original synthetic pixel art, only for isolated debug fixtures/tests.
use super::*;
use std::io::Write;

pub fn bytes() -> Vec<u8> {
    let mut main = image::RgbImage::from_pixel(275, 116, image::Rgb([29, 35, 42]));
    for (x, y, pixel) in main.enumerate_pixels_mut() {
        if x == 0 || x == 274 || y == 0 || y == 115 {
            *pixel = image::Rgb([109, 150, 159]);
        }
        if y > 1 && y < 13 && x > 1 && x < 274 {
            *pixel = image::Rgb([47, 77, 88]);
        }
    }
    let mut buttons = image::RgbImage::from_pixel(136, 36, image::Rgb([31, 48, 55]));
    for i in 0..5u32 {
        for row in 0..2u32 {
            let ox = i * 23;
            let oy = row * 18;
            for y in 1..17 {
                for x in 1..22 {
                    let color = if row == 0 {
                        [77, 117, 126]
                    } else {
                        [41, 73, 85]
                    };
                    buttons.put_pixel(ox + x, oy + y, image::Rgb(color));
                }
            }
            for y in 4..14 {
                for x in 5..18 {
                    let mark = match i {
                        0 => {
                            (x == 5)
                                || ((8..=15).contains(&x) && (y as i32 - 9).abs() <= x as i32 - 8)
                        }
                        1 => x >= 7 && (y as i32 - 9).abs() <= 17 - x as i32,
                        2 => (7..=9).contains(&x) || (13..=15).contains(&x),
                        3 => (7..=15).contains(&x) && (5..=12).contains(&y),
                        _ => {
                            (x == 17)
                                || ((6..=13).contains(&x) && (y as i32 - 9).abs() <= 13 - x as i32)
                        }
                    };
                    if mark {
                        buttons.put_pixel(ox + x, oy + y, image::Rgb([230, 250, 246]));
                    }
                }
            }
        }
    }
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, image) in [("main.bmp", main), ("cbuttons.bmp", buttons)] {
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, image::ImageFormat::Bmp)
            .unwrap();
        writer
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(&bytes.into_inner()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
