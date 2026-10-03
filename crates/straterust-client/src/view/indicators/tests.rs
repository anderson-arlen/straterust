use super::*;

#[test]
fn segmented_bars_preserve_gaps_at_fractional_origins_zoom_and_dpi() {
    let colors = std::array::from_fn(|index| if index == 18 { 0 } else { 0x00ff00 });
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for zoom in [1.0, 1.2, 1.5, 2.0] {
            for offset in [0.0, 0.25, 0.5, 0.75] {
                let mut pixels = vec![0xffffff; 200 * 40];
                let origin = [10.0 + offset, 5.0 + offset];
                let mut canvas = Canvas {
                    scene: None,
                    pixels: &mut pixels,
                    width: 200,
                    height: 40,
                    scale,
                };
                draw_bar(&mut canvas, origin, 19, 1.0, &colors, 0, zoom);
                let y = ((origin[1] + 2.0 * zoom) * scale).round() as usize;
                for x in (3..18).step_by(3) {
                    let left = ((origin[0] + f64::from(x) * zoom) * scale).round() as usize;
                    let right = ((origin[0] + f64::from(x + 1) * zoom) * scale).round() as usize;
                    assert!(right > left);
                    assert!(
                        pixels[y * 200 + left..y * 200 + right]
                            .iter()
                            .all(|pixel| *pixel == 0),
                        "solid bar at zoom={zoom} dpi={scale} offset={offset}"
                    );
                    assert_eq!(pixels[y * 200 + right], 0x00ff00);
                }
            }
        }
    }
}
