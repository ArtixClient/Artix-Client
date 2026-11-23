//! Skin download / decode / upload through the official Mojang endpoints.
use base64::Engine;
use serde_json::Value;

use crate::net::{self, R};

pub struct Skin {
    /// 64x64 RGBA
    pub rgba: Vec<u8>,
    pub slim: bool,
}

pub fn blank() -> Skin {
    Skin { rgba: vec![0; 64 * 64 * 4], slim: false }
}

pub fn decode_png(bytes: &[u8]) -> R<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(bytes);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        _ => return Err("Unsupported PNG type".into()),
    };
    Ok((info.width, info.height, rgba))
}

pub fn encode_png(rgba: &[u8], w: u32, h: u32) -> R<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

fn copy_flip(img: &mut [u8], src: (usize, usize, usize, usize), dst: (usize, usize)) {
    let (sx, sy, w, h) = src;
    for y in 0..h {
        for x in 0..w {
            let s = ((sy + y) * 64 + sx + (w - 1 - x)) * 4;
            let d = ((dst.1 + y) * 64 + dst.0 + x) * 4;
            let px = [img[s], img[s + 1], img[s + 2], img[s + 3]];
            img[d..d + 4].copy_from_slice(&px);
        }
    }
}

/// Accepts 64x64 or legacy 64x32 skins and returns a 64x64 RGBA image.
pub fn to_64(w: u32, h: u32, rgba: Vec<u8>) -> R<Vec<u8>> {
    if w != 64 || (h != 64 && h != 32) {
        return Err(format!("Skins must be 64x64 (or 64x32), this image is {w}x{h}"));
    }
    if h == 64 {
        return Ok(rgba);
    }
    let mut img = vec![0u8; 64 * 64 * 4];
    img[..rgba.len()].copy_from_slice(&rgba);
    // mirror the right leg (0,16) and right arm (40,16) onto the left ones
    for (ox, oy, dx, dy, bw) in [(0usize, 16usize, 16usize, 48usize, 4usize), (40, 16, 32, 48, 4)] {
        copy_flip(&mut img, (ox + 4, oy, bw, 4), (dx + 4, dy));
        copy_flip(&mut img, (ox + 8, oy, bw, 4), (dx + 8, dy));
        copy_flip(&mut img, (ox + 8, oy + 4, 4, 12), (dx, dy + 4));
        copy_flip(&mut img, (ox + 4, oy + 4, bw, 12), (dx + 4, dy + 4));
        copy_flip(&mut img, (ox, oy + 4, 4, 12), (dx + 8, dy + 4));
        copy_flip(&mut img, (ox + 12, oy + 4, bw, 12), (dx + 12, dy + 4));
    }
    Ok(img)
}

fn skin_from_url(url: &str, slim: bool) -> R<Skin> {
    let url = url.replacen("http://", "https://", 1);
    let (w, h, px) = decode_png(&net::get_bytes(&url)?)?;
    Ok(Skin { rgba: to_64(w, h, px)?, slim })
}

pub fn from_profile(profile: &Value) -> R<Skin> {
    let skins = profile["skins"].as_array().cloned().unwrap_or_default();
    match skins.iter().find(|s| s["state"].as_str() == Some("ACTIVE")).or(skins.first()) {
        Some(s) => skin_from_url(s["url"].as_str().ok_or("no skin url")?, s["variant"].as_str() == Some("SLIM")),
        None => Ok(blank()),
    }
}

pub fn from_username(name: &str) -> R<Skin> {
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("Enter a valid Minecraft username".into());
    }
    let p = net::get_value(&format!("https://api.mojang.com/users/profiles/minecraft/{name}")).map_err(|_| "Player not found".to_string())?;
    let id = p["id"].as_str().ok_or("Player not found")?;
    let prof = net::get_value(&format!("https://sessionserver.mojang.com/session/minecraft/profile/{id}"))?;
    let val = prof["properties"].as_array().and_then(|a| a.iter().find(|x| x["name"] == "textures")).and_then(|x| x["value"].as_str()).ok_or("No skin data")?;
    let dec = base64::engine::general_purpose::STANDARD.decode(val).map_err(|e| e.to_string())?;
    let tex: Value = serde_json::from_slice(&dec).map_err(|e| e.to_string())?;
    let skin = &tex["textures"]["SKIN"];
    match skin["url"].as_str() {
        Some(u) => skin_from_url(u, skin["metadata"]["model"].as_str() == Some("slim")),
        None => Ok(blank()),
    }
}

pub fn upload(token: &str, png_bytes: &[u8], slim: bool) -> R<()> {
    let b = "----ArtixLauncherBoundary7MA4YWxkTrZu0gW";
    let mut body: Vec<u8> = Vec::new();
    body.extend(format!("--{b}\r\nContent-Disposition: form-data; name=\"variant\"\r\n\r\n{}\r\n", if slim { "slim" } else { "classic" }).as_bytes());
    body.extend(format!("--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"skin.png\"\r\nContent-Type: image/png\r\n\r\n").as_bytes());
    body.extend(png_bytes);
    body.extend(format!("\r\n--{b}--\r\n").as_bytes());
    let (st, _) = net::post_bytes("https://api.minecraftservices.com/minecraft/profile/skins", &format!("multipart/form-data; boundary={b}"), &body, token)?;
    if st == 200 { Ok(()) } else { Err(format!("Mojang rejected the skin (HTTP {st}). Check it is a valid 64x64 PNG.")) }
}

pub fn set_cape(token: &str, id: Option<&str>) -> R<()> {
    let st = match id {
        Some(id) => net::put_json("https://api.minecraftservices.com/minecraft/profile/capes/active", &serde_json::json!({ "capeId": id }), token)?.0,
        None => net::delete("https://api.minecraftservices.com/minecraft/profile/capes/active", token)?,
    };
    if st == 200 || st == 204 { Ok(()) } else { Err(format!("Cape change failed (HTTP {st})")) }
}
