/// ALiS v1 binary protocol encoder
///
/// Specification: https://docs.asciinema.org/manual/alis/v1/
use anyhow::{Context, Result};

/// ALiS magic string and version
pub const ALIS_MAGIC: &[u8] = b"ALiS\x01";

/// ALiS event type codes
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    Init = 0x01,
    EOT = 0x04,     // End of Transmission
    Output = 0x6F,  // 'o'
    Input = 0x69,   // 'i'
    Resize = 0x72,  // 'r'
    Marker = 0x6D,  // 'm'
    Exit = 0x78,    // 'x'
}

/// Theme format codes
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeFormat {
    None = 0x00,
    Palette8 = 0x08,
    Palette16 = 0x10,
}

/// Encode unsigned integer as LEB128
pub fn encode_leb128(value: u64) -> Vec<u8> {
    let mut result = Vec::new();
    let mut val = value;

    loop {
        let mut byte = (val & 0x7F) as u8;
        val >>= 7;

        if val != 0 {
            byte |= 0x80;
        }

        result.push(byte);

        if val == 0 {
            break;
        }
    }

    result
}

/// Encode a string with length prefix
pub fn encode_string(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut result = encode_leb128(bytes.len() as u64);
    result.extend_from_slice(bytes);
    result
}

/// Theme configuration for encoding
#[derive(Debug, Clone)]
pub struct Theme {
    pub fg: String,
    pub bg: String,
    pub palette: Vec<String>,
}

/// Encode theme
pub fn encode_theme(theme: Option<&Theme>) -> Result<Vec<u8>> {
    let mut result = Vec::new();

    match theme {
        None => {
            result.push(ThemeFormat::None as u8);
        }
        Some(theme) => {
            let palette_len = theme.palette.len();

            if palette_len == 0 {
                result.push(ThemeFormat::None as u8);
            } else if palette_len <= 8 {
                result.push(ThemeFormat::Palette8 as u8);

                // Encode fg and bg colors
                let fg_rgb = parse_color(&theme.fg)?;
                let bg_rgb = parse_color(&theme.bg)?;
                result.extend_from_slice(&fg_rgb);
                result.extend_from_slice(&bg_rgb);

                // Encode palette (8 colors)
                for i in 0..8 {
                    if i < palette_len {
                        let rgb = parse_color(&theme.palette[i])?;
                        result.extend_from_slice(&rgb);
                    } else {
                        result.extend_from_slice(&[0, 0, 0]);
                    }
                }
            } else {
                result.push(ThemeFormat::Palette16 as u8);

                // Encode fg and bg colors
                let fg_rgb = parse_color(&theme.fg)?;
                let bg_rgb = parse_color(&theme.bg)?;
                result.extend_from_slice(&fg_rgb);
                result.extend_from_slice(&bg_rgb);

                // Encode palette (16 colors)
                for i in 0..16 {
                    if i < palette_len {
                        let rgb = parse_color(&theme.palette[i])?;
                        result.extend_from_slice(&rgb);
                    } else {
                        result.extend_from_slice(&[0, 0, 0]);
                    }
                }
            }
        }
    }

    Ok(result)
}

/// Parse color string (#RRGGBB) to RGB bytes
fn parse_color(color: &str) -> Result<[u8; 3]> {
    let color = color.trim_start_matches('#');

    if color.len() != 6 {
        anyhow::bail!("invalid color format: {}", color);
    }

    let r = u8::from_str_radix(&color[0..2], 16).context("invalid red component")?;
    let g = u8::from_str_radix(&color[2..4], 16).context("invalid green component")?;
    let b = u8::from_str_radix(&color[4..6], 16).context("invalid blue component")?;

    Ok([r, g, b])
}

/// Encode Init event
pub fn encode_init(
    last_id: u64,
    rel_time: u64,
    cols: u16,
    rows: u16,
    theme: Option<&Theme>,
    init_data: &str,
) -> Result<Vec<u8>> {
    let mut buf = Vec::new();

    buf.push(EventType::Init as u8);
    buf.extend_from_slice(&encode_leb128(last_id));
    buf.extend_from_slice(&encode_leb128(rel_time));
    buf.extend_from_slice(&encode_leb128(cols as u64));
    buf.extend_from_slice(&encode_leb128(rows as u64));
    buf.extend_from_slice(&encode_theme(theme)?);
    buf.extend_from_slice(&encode_string(init_data));

    Ok(buf)
}

/// Encode Output event
pub fn encode_output(id: u64, rel_time: u64, data: &str) -> Vec<u8> {
    let mut buf = Vec::new();

    buf.push(EventType::Output as u8);
    buf.extend_from_slice(&encode_leb128(id));
    buf.extend_from_slice(&encode_leb128(rel_time));
    buf.extend_from_slice(&encode_string(data));

    buf
}

/// Encode Input event
pub fn encode_input(id: u64, rel_time: u64, data: &str) -> Vec<u8> {
    let mut buf = Vec::new();

    buf.push(EventType::Input as u8);
    buf.extend_from_slice(&encode_leb128(id));
    buf.extend_from_slice(&encode_leb128(rel_time));
    buf.extend_from_slice(&encode_string(data));

    buf
}

/// Encode Resize event
pub fn encode_resize(id: u64, rel_time: u64, cols: u16, rows: u16) -> Vec<u8> {
    let mut buf = Vec::new();

    buf.push(EventType::Resize as u8);
    buf.extend_from_slice(&encode_leb128(id));
    buf.extend_from_slice(&encode_leb128(rel_time));
    buf.extend_from_slice(&encode_leb128(cols as u64));
    buf.extend_from_slice(&encode_leb128(rows as u64));

    buf
}

/// Encode Marker event
pub fn encode_marker(id: u64, rel_time: u64, label: &str) -> Vec<u8> {
    let mut buf = Vec::new();

    buf.push(EventType::Marker as u8);
    buf.extend_from_slice(&encode_leb128(id));
    buf.extend_from_slice(&encode_leb128(rel_time));
    buf.extend_from_slice(&encode_string(label));

    buf
}

/// Encode Exit event
pub fn encode_exit(id: u64, rel_time: u64, status: i32) -> Vec<u8> {
    let mut buf = Vec::new();

    buf.push(EventType::Exit as u8);
    buf.extend_from_slice(&encode_leb128(id));
    buf.extend_from_slice(&encode_leb128(rel_time));
    buf.extend_from_slice(&encode_leb128(status as u64));

    buf
}

/// Encode EOT (End of Transmission) event
///
/// This event signals the end of a stream without closing the WebSocket connection.
/// Useful for persistent connections across session restarts.
pub fn encode_eot(id: u64, rel_time: u64) -> Vec<u8> {
    let mut buf = Vec::new();

    buf.push(EventType::EOT as u8);
    buf.extend_from_slice(&encode_leb128(id));
    buf.extend_from_slice(&encode_leb128(rel_time));

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leb128_encoding() {
        assert_eq!(encode_leb128(0), vec![0x00]);
        assert_eq!(encode_leb128(1), vec![0x01]);
        assert_eq!(encode_leb128(127), vec![0x7F]);
        assert_eq!(encode_leb128(128), vec![0x80, 0x01]);
        assert_eq!(encode_leb128(300), vec![0xAC, 0x02]);
        assert_eq!(encode_leb128(16384), vec![0x80, 0x80, 0x01]);
    }

    #[test]
    fn test_string_encoding() {
        assert_eq!(encode_string(""), vec![0x00]);
        assert_eq!(encode_string("a"), vec![0x01, b'a']);
        assert_eq!(encode_string("hello"), vec![0x05, b'h', b'e', b'l', b'l', b'o']);
    }

    #[test]
    fn test_color_parsing() {
        assert_eq!(parse_color("#000000").unwrap(), [0, 0, 0]);
        assert_eq!(parse_color("#FFFFFF").unwrap(), [255, 255, 255]);
        assert_eq!(parse_color("#FF0000").unwrap(), [255, 0, 0]);
        assert_eq!(parse_color("#00FF00").unwrap(), [0, 255, 0]);
        assert_eq!(parse_color("#0000FF").unwrap(), [0, 0, 255]);
        assert_eq!(parse_color("#123456").unwrap(), [0x12, 0x34, 0x56]);
    }

    #[test]
    fn test_theme_none_encoding() {
        let encoded = encode_theme(None).unwrap();
        assert_eq!(encoded, vec![0x00]);
    }

    #[test]
    fn test_output_event_encoding() {
        let encoded = encode_output(1, 1000, "hello");
        assert_eq!(encoded[0], EventType::Output as u8);
        assert_eq!(encoded[1], 0x01); // id = 1
        assert_eq!(encoded[2], 0xE8); // rel_time = 1000
        assert_eq!(encoded[3], 0x07);
        assert_eq!(encoded[4], 0x05); // string length = 5
        assert_eq!(&encoded[5..], b"hello");
    }

    #[test]
    fn test_resize_event_encoding() {
        let encoded = encode_resize(2, 500, 80, 24);
        assert_eq!(encoded[0], EventType::Resize as u8);
        assert_eq!(encoded[1], 0x02); // id = 2
        assert_eq!(encoded[2], 0xF4); // rel_time = 500
        assert_eq!(encoded[3], 0x03);
        assert_eq!(encoded[4], 0x50); // cols = 80
        assert_eq!(encoded[5], 0x18); // rows = 24
    }

    #[test]
    fn test_marker_event_encoding() {
        let encoded = encode_marker(3, 100, "chapter 1");
        assert_eq!(encoded[0], EventType::Marker as u8);
        assert_eq!(encoded[1], 0x03); // id = 3
        assert_eq!(encoded[2], 0x64); // rel_time = 100
        assert_eq!(encoded[3], 0x09); // string length = 9
        assert_eq!(&encoded[4..], b"chapter 1");
    }

    #[test]
    fn test_exit_event_encoding() {
        let encoded = encode_exit(4, 200, 0);
        assert_eq!(encoded[0], EventType::Exit as u8);
        assert_eq!(encoded[1], 0x04); // id = 4
        assert_eq!(encoded[2], 0xC8); // rel_time = 200
        assert_eq!(encoded[3], 0x01);
        assert_eq!(encoded[4], 0x00); // status = 0
    }

    #[test]
    fn test_eot_event_encoding() {
        let encoded = encode_eot(5, 300);
        assert_eq!(encoded[0], EventType::EOT as u8);
        assert_eq!(encoded[1], 0x05); // id = 5
        assert_eq!(encoded[2], 0xAC); // rel_time = 300 (0xAC, 0x02 in LEB128)
        assert_eq!(encoded[3], 0x02);
        assert_eq!(encoded.len(), 4); // No data payload
    }

    #[test]
    fn test_init_event_encoding() {
        let encoded = encode_init(0, 0, 80, 24, None, "test").unwrap();
        assert_eq!(encoded[0], EventType::Init as u8);
        assert_eq!(encoded[1], 0x00); // last_id = 0
        assert_eq!(encoded[2], 0x00); // rel_time = 0
        assert_eq!(encoded[3], 0x50); // cols = 80
        assert_eq!(encoded[4], 0x18); // rows = 24
        assert_eq!(encoded[5], 0x00); // theme format = none
        assert_eq!(encoded[6], 0x04); // string length = 4
        assert_eq!(&encoded[7..], b"test");
    }
}
