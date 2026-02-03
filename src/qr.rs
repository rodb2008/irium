use qrcode::types::Color;
use qrcode::QrCode;

fn qr_code(data: &str) -> Result<QrCode, String> {
    QrCode::new(data.as_bytes()).map_err(|e| format!("qr encode failed: {e}"))
}

pub fn render_ascii(data: &str) -> Result<String, String> {
    let code = qr_code(data)?;
    let width = code.width();
    let colors = code.to_colors();
    let border = 1usize;
    let mut out = String::new();
    let rows = width + border * 2;
    for y in 0..rows {
        for x in 0..rows {
            let dark = if x >= border && y >= border && x < width + border && y < width + border {
                let idx = (y - border) * width + (x - border);
                colors
                    .get(idx)
                    .map(|c| matches!(c, Color::Dark))
                    .unwrap_or(false)
            } else {
                false
            };
            if dark {
                out.push_str("##");
            } else {
                out.push_str("  ");
            }
        }
        out.push_str("\n");
    }
    Ok(out)
}

pub fn render_svg(data: &str, module_px: u32, margin: u32) -> Result<String, String> {
    let code = qr_code(data)?;
    let width = code.width() as u32;
    let colors = code.to_colors();
    let module_px = module_px.clamp(1, 64);
    let margin = margin.clamp(0, 16);
    let size = (width + margin * 2) * module_px;

    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{s}\" height=\"{s}\" viewBox=\"0 0 {s} {s}\" shape-rendering=\"crispEdges\">",
        s = size
    ));
    out.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#fff\"/>");

    for y in 0..width {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            if colors
                .get(idx)
                .map(|c| matches!(c, Color::Dark))
                .unwrap_or(false)
            {
                let rx = (x + margin) * module_px;
                let ry = (y + margin) * module_px;
                out.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#000\"/>",
                    rx, ry, module_px, module_px
                ));
            }
        }
    }
    out.push_str("</svg>");
    Ok(out)
}
