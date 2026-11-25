//! Flutter Color & Material/Cupertino Icon catalogs and source scanners.

use crate::{LineIndex, TextSpan};

#[derive(Clone, Debug, PartialEq)]
pub struct FlutterColorMatch {
    pub span: TextSpan,
    pub label: String,
    pub argb: u32,
}

impl FlutterColorMatch {
    pub fn rgba_f32(&self) -> (f32, f32, f32, f32) {
        let a = ((self.argb >> 24) & 0xff) as f32 / 255.0;
        let r = ((self.argb >> 16) & 0xff) as f32 / 255.0;
        let g = ((self.argb >> 8) & 0xff) as f32 / 255.0;
        let b = (self.argb & 0xff) as f32 / 255.0;
        (r, g, b, a)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconEntry {
    pub family: &'static str,
    pub name: &'static str,
    pub codepoint: u32,
    pub category: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlutterIconMatch {
    pub span: TextSpan,
    pub full_name: String,
    pub entry: Option<&'static IconEntry>,
}

/// Scans `Color(0x...)`, `Color.fromARGB(...)`, `Color.fromRGBO(...)`, `Colors.*`, and `CupertinoColors.*`.
pub fn find_flutter_colors(text: &str) -> Vec<FlutterColorMatch> {
    let idx = LineIndex::new(text);
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut matches = Vec::new();

    'outer: while i < len {
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let q = bytes[i];
            i += 1;
            while i < len && bytes[i] != q {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(len);
            continue;
        }

        if let Some(rest) = text[i..].strip_prefix("Color(")
            && is_boundary(bytes, i)
            && let Some(close) = rest.find(')')
        {
            let inner = rest[..close].trim();
            if let Some(hex) = inner.strip_prefix("0x").or_else(|| inner.strip_prefix("0X"))
                && let Ok(argb) = u32::from_str_radix(&hex.replace('_', ""), 16)
            {
                let end = i + 6 + close + 1;
                matches.push(FlutterColorMatch {
                    span: idx.span(text, i, end),
                    label: format!("Color(0x{argb:08X})"),
                    argb,
                });
                i = end;
                continue;
            }
        }

        if let Some(rest) = text[i..].strip_prefix("Color.fromARGB(")
            && is_boundary(bytes, i)
            && let Some(close) = rest.find(')')
        {
            let parts: Vec<_> = rest[..close].split(',').map(|s| s.trim().parse::<u8>()).collect();
            if let [Ok(a), Ok(r), Ok(g), Ok(b)] = parts.as_slice() {
                let end = i + 15 + close + 1;
                let argb = ((*a as u32) << 24) | ((*r as u32) << 16) | ((*g as u32) << 8) | (*b as u32);
                matches.push(FlutterColorMatch {
                    span: idx.span(text, i, end),
                    label: format!("Color.fromARGB({a}, {r}, {g}, {b})"),
                    argb,
                });
                i = end;
                continue;
            }
        }

        if let Some(rest) = text[i..].strip_prefix("Color.fromRGBO(")
            && is_boundary(bytes, i)
            && let Some(close) = rest.find(')')
        {
            let raw: Vec<_> = rest[..close].split(',').map(str::trim).collect();
            if raw.len() == 4
                && let (Ok(r), Ok(g), Ok(b), Ok(o)) = (
                    raw[0].parse::<u8>(),
                    raw[1].parse::<u8>(),
                    raw[2].parse::<u8>(),
                    raw[3].parse::<f32>(),
                )
            {
                let a = (o.clamp(0.0, 1.0) * 255.0).round() as u8;
                let end = i + 15 + close + 1;
                let argb = ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
                matches.push(FlutterColorMatch {
                    span: idx.span(text, i, end),
                    label: format!("Color.fromRGBO({r}, {g}, {b}, {o})"),
                    argb,
                });
                i = end;
                continue;
            }
        }

        for prefix in ["Colors.", "CupertinoColors."] {
            if text[i..].starts_with(prefix) && is_boundary(bytes, i) {
                let mut end = i + prefix.len();
                while end < len && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                    end += 1;
                }
                // Optional `.shade500` or `[500]`
                let mut full_end = end;
                if text[end..].starts_with(".shade") {
                    let mut s_end = end + 6;
                    while s_end < len && bytes[s_end].is_ascii_digit() {
                        s_end += 1;
                    }
                    full_end = s_end;
                } else if text[end..].starts_with('[')
                    && let Some(rb) = text[end + 1..].find(']')
                {
                    full_end = end + 1 + rb + 1;
                }
                let expr = &text[i..full_end];
                if let Some(argb) = lookup_named_color(expr) {
                    matches.push(FlutterColorMatch {
                        span: idx.span(text, i, full_end),
                        label: expr.to_owned(),
                        argb,
                    });
                    i = full_end;
                    continue 'outer;
                }
            }
        }
        i += 1;
    }
    matches
}

pub fn format_color_presentations(r: f32, g: f32, b: f32, a: f32) -> Vec<String> {
    let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (au, ru, gu, bu) = (to_u8(a), to_u8(r), to_u8(g), to_u8(b));
    let argb = ((au as u32) << 24) | ((ru as u32) << 16) | ((gu as u32) << 8) | (bu as u32);
    vec![
        format!("Color(0x{argb:08X})"),
        format!("Color.fromARGB({au}, {ru}, {gu}, {bu})"),
        format!("Color.fromRGBO({ru}, {gu}, {bu}, {:.2})", a.clamp(0.0, 1.0)),
    ]
}

pub fn lookup_named_color(expr: &str) -> Option<u32> {
    if let Some(name) = expr.strip_prefix("CupertinoColors.") {
        return CUPERTINO_COLORS.iter().find(|(n, _)| *n == name).map(|(_, v)| *v);
    }
    let rest = expr.strip_prefix("Colors.")?;
    if let Some((base, shade_str)) = rest.split_once(".shade") {
        let shade: u16 = shade_str.parse().ok()?;
        return material_shade(base, shade);
    }
    if let Some((base, idx)) = rest.split_once('[')
        && let Some(shade_str) = idx.strip_suffix(']')
    {
        let shade: u16 = shade_str.parse().ok()?;
        return material_shade(base, shade);
    }
    MATERIAL_COLORS.iter().find(|(n, _)| *n == rest).map(|(_, v)| *v)
}

fn material_shade(base: &str, shade: u16) -> Option<u32> {
    let table: &[u32; 10] = match base {
        "red" => &[0xFFFFEBEE, 0xFFFFCDD2, 0xFFEF9A9A, 0xFFE57373, 0xFFEF5350, 0xFFF44336, 0xFFE53935, 0xFFD32F2F, 0xFFC62828, 0xFFB71C1C],
        "pink" => &[0xFFFCE4EC, 0xFFF8BBD0, 0xFFF48FB1, 0xFFF06292, 0xFFEC407A, 0xFFE91E63, 0xFFD81B60, 0xFFC2185B, 0xFFAD1457, 0xFF880E4F],
        "purple" => &[0xFFF3E5F5, 0xFFE1BEE7, 0xFFCE93D8, 0xFFBA68C8, 0xFFAB47BC, 0xFF9C27B0, 0xFF8E24AA, 0xFF7B1FA2, 0xFF6A1B9A, 0xFF4A148C],
        "deepPurple" => &[0xFFEDE7F6, 0xFFD1C4E9, 0xFFB39DDB, 0xFF9575CD, 0xFF7E57C2, 0xFF673AB7, 0xFF5E35B1, 0xFF512DA8, 0xFF4527A0, 0xFF311B92],
        "indigo" => &[0xFFE8EAF6, 0xFFC5CAE9, 0xFF9FA8DA, 0xFF7986CB, 0xFF5C6BC0, 0xFF3F51B5, 0xFF3949AB, 0xFF303F9F, 0xFF283593, 0xFF1A237E],
        "blue" => &[0xFFE3F2FD, 0xFFBBDEFB, 0xFF90CAF9, 0xFF64B5F6, 0xFF42A5F5, 0xFF2196F3, 0xFF1E88E5, 0xFF1976D2, 0xFF1565C0, 0xFF0D47A1],
        "lightBlue" => &[0xFFE1F5FE, 0xFFB3E5FC, 0xFF81D4FA, 0xFF4FC3F7, 0xFF29B6F6, 0xFF03A9F4, 0xFF039BE5, 0xFF0288D1, 0xFF0277BD, 0xFF01579B],
        "cyan" => &[0xFFE0F7FA, 0xFFB2EBF2, 0xFF80DEEA, 0xFF4DD0E1, 0xFF26C6DA, 0xFF00BCD4, 0xFF00ACC1, 0xFF0097A7, 0xFF00838F, 0xFF006064],
        "teal" => &[0xFFE0F2F1, 0xFFB2DFDB, 0xFF80CBC4, 0xFF4DB6AC, 0xFF26A69A, 0xFF009688, 0xFF00897B, 0xFF00796B, 0xFF00695C, 0xFF004D40],
        "green" => &[0xFFE8F5E9, 0xFFC8E6C9, 0xFFA5D6A7, 0xFF81C784, 0xFF66BB6A, 0xFF4CAF50, 0xFF43A047, 0xFF388E3C, 0xFF2E7D32, 0xFF1B5E20],
        "amber" => &[0xFFFFF8E1, 0xFFFFECB3, 0xFFFFE082, 0xFFFFD54F, 0xFFFFCA28, 0xFFFFC107, 0xFFFFB300, 0xFFFFA000, 0xFFFF8F00, 0xFFFF6F00],
        "orange" => &[0xFFFFF3E0, 0xFFFFE0B2, 0xFFFFCC80, 0xFFFFB74D, 0xFFFFA726, 0xFFFF9800, 0xFFFB8C00, 0xFFF57C00, 0xFFEF6C00, 0xFFE65100],
        "grey" => &[0xFFFAFAFA, 0xFFF5F5F5, 0xFFEEEEEE, 0xFFE0E0E0, 0xFFBDBDBD, 0xFF9E9E9E, 0xFF757575, 0xFF616161, 0xFF424242, 0xFF212121],
        "blueGrey" => &[0xFFECEFF1, 0xFFCFD8DC, 0xFFB0BEC5, 0xFF90A4AE, 0xFF78909C, 0xFF607D8B, 0xFF546E7A, 0xFF455A64, 0xFF37474F, 0xFF263238],
        _ => return None,
    };
    let pos = match shade {
        50 => 0,
        100 => 1,
        200 => 2,
        300 => 3,
        400 => 4,
        500 => 5,
        600 => 6,
        700 => 7,
        800 => 8,
        900 => 9,
        _ => return None,
    };
    Some(table[pos])
}

/// Scans `Icons.<name>` and `CupertinoIcons.<name>` references in Dart source.
pub fn find_flutter_icons(text: &str) -> Vec<FlutterIconMatch> {
    let idx = LineIndex::new(text);
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut out = Vec::new();
    'outer: while i < len {
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        for (family, prefix) in [("Material", "Icons."), ("Cupertino", "CupertinoIcons.")] {
            if text[i..].starts_with(prefix) && is_boundary(bytes, i) {
                let mut end = i + prefix.len();
                while end < len && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                    end += 1;
                }
                if end > i + prefix.len() {
                    let name = &text[i + prefix.len()..end];
                    let entry = FLUTTER_ICONS
                        .iter()
                        .find(|e| e.family == family && e.name == name);
                    out.push(FlutterIconMatch {
                        span: idx.span(text, i, end),
                        full_name: text[i..end].to_owned(),
                        entry,
                    });
                    i = end;
                    continue 'outer;
                }
            }
        }
        i += 1;
    }
    out
}

fn is_boundary(bytes: &[u8], i: usize) -> bool {
    i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')
}

pub const MATERIAL_COLORS: &[(&str, u32)] = &[
    ("transparent", 0x00000000),
    ("black", 0xFF000000),
    ("black87", 0xDD000000),
    ("black54", 0x8A000000),
    ("black45", 0x73000000),
    ("black38", 0x61000000),
    ("black26", 0x42000000),
    ("black12", 0x1F000000),
    ("white", 0xFFFFFFFF),
    ("white70", 0xB3FFFFFF),
    ("white60", 0x99FFFFFF),
    ("white54", 0x8AFFFFFF),
    ("white38", 0x62FFFFFF),
    ("white30", 0x4DFFFFFF),
    ("white24", 0x3DFFFFFF),
    ("white12", 0x1FFFFFFF),
    ("white10", 0x1AFFFFFF),
    ("red", 0xFFF44336),
    ("redAccent", 0xFFFF5252),
    ("pink", 0xFFE91E63),
    ("pinkAccent", 0xFFFF4081),
    ("purple", 0xFF9C27B0),
    ("purpleAccent", 0xFFE040FB),
    ("deepPurple", 0xFF673AB7),
    ("deepPurpleAccent", 0xFF7C4DFF),
    ("indigo", 0xFF3F51B5),
    ("indigoAccent", 0xFF536DFE),
    ("blue", 0xFF2196F3),
    ("blueAccent", 0xFF448AFF),
    ("lightBlue", 0xFF03A9F4),
    ("lightBlueAccent", 0xFF40C4FF),
    ("cyan", 0xFF00BCD4),
    ("cyanAccent", 0xFF18FFFF),
    ("teal", 0xFF009688),
    ("tealAccent", 0xFF64FFDA),
    ("green", 0xFF4CAF50),
    ("greenAccent", 0xFF69F0AE),
    ("lightGreen", 0xFF8BC34A),
    ("lightGreenAccent", 0xFFB2FF59),
    ("lime", 0xFFCDDC39),
    ("limeAccent", 0xFFEEFF41),
    ("yellow", 0xFFFFEB3B),
    ("yellowAccent", 0xFFFFFF00),
    ("amber", 0xFFFFC107),
    ("amberAccent", 0xFFFFD740),
    ("orange", 0xFFFF9800),
    ("orangeAccent", 0xFFFFAB40),
    ("deepOrange", 0xFFFF5722),
    ("deepOrangeAccent", 0xFFFF6E40),
    ("brown", 0xFF795548),
    ("grey", 0xFF9E9E9E),
    ("blueGrey", 0xFF607D8B),
];

pub const CUPERTINO_COLORS: &[(&str, u32)] = &[
    ("activeBlue", 0xFF007AFF),
    ("activeGreen", 0xFF34C759),
    ("activeOrange", 0xFFFF9500),
    ("destructiveRed", 0xFFFF3B30),
    ("systemRed", 0xFFFF3B30),
    ("systemGreen", 0xFF34C759),
    ("systemBlue", 0xFF007AFF),
    ("systemOrange", 0xFFFF9500),
    ("systemYellow", 0xFFFFCC00),
    ("systemPink", 0xFFFF2D55),
    ("systemPurple", 0xFFAF52DE),
    ("systemTeal", 0xFF5AC8FA),
    ("systemIndigo", 0xFF5856D6),
    ("systemGrey", 0xFF8E8E93),
    ("systemGrey2", 0xFFAEAEB2),
    ("systemGrey3", 0xFFC7C7CC),
    ("systemGrey4", 0xFFD1D1D6),
    ("systemGrey5", 0xFFE5E5EA),
    ("systemGrey6", 0xFFF2F2F7),
    ("white", 0xFFFFFFFF),
    ("black", 0xFF000000),
    ("label", 0xFF000000),
    ("secondaryLabel", 0x993C3C43),
    ("systemBackground", 0xFFFFFFFF),
    ("secondarySystemBackground", 0xFFF2F2F7),
];

pub const FLUTTER_ICONS: &[IconEntry] = &[
    IconEntry { family: "Material", name: "add", codepoint: 0xe047, category: "Action" },
    IconEntry { family: "Material", name: "add_circle", codepoint: 0xe048, category: "Content" },
    IconEntry { family: "Material", name: "add_circle_outline", codepoint: 0xe049, category: "Content" },
    IconEntry { family: "Material", name: "alarm", codepoint: 0xe059, category: "Action" },
    IconEntry { family: "Material", name: "apps", codepoint: 0xe08f, category: "Navigation" },
    IconEntry { family: "Material", name: "arrow_back", codepoint: 0xe092, category: "Navigation" },
    IconEntry { family: "Material", name: "arrow_back_ios", codepoint: 0xe093, category: "Navigation" },
    IconEntry { family: "Material", name: "arrow_downward", codepoint: 0xe095, category: "Navigation" },
    IconEntry { family: "Material", name: "arrow_forward", codepoint: 0xe09b, category: "Navigation" },
    IconEntry { family: "Material", name: "arrow_forward_ios", codepoint: 0xe09c, category: "Navigation" },
    IconEntry { family: "Material", name: "arrow_upward", codepoint: 0xe0a0, category: "Navigation" },
    IconEntry { family: "Material", name: "attach_file", codepoint: 0xe0b1, category: "Editor" },
    IconEntry { family: "Material", name: "backspace", codepoint: 0xe0c4, category: "Content" },
    IconEntry { family: "Material", name: "bookmark", codepoint: 0xe0e8, category: "Action" },
    IconEntry { family: "Material", name: "bookmark_border", codepoint: 0xe0eb, category: "Action" },
    IconEntry { family: "Material", name: "bug_report", codepoint: 0xe10c, category: "Action" },
    IconEntry { family: "Material", name: "build", codepoint: 0xe10d, category: "Action" },
    IconEntry { family: "Material", name: "calendar_today", codepoint: 0xe122, category: "Action" },
    IconEntry { family: "Material", name: "call", codepoint: 0xe126, category: "Communication" },
    IconEntry { family: "Material", name: "camera_alt", codepoint: 0xe130, category: "Image" },
    IconEntry { family: "Material", name: "cancel", codepoint: 0xe139, category: "Navigation" },
    IconEntry { family: "Material", name: "chat", codepoint: 0xe153, category: "Communication" },
    IconEntry { family: "Material", name: "check", codepoint: 0xe156, category: "Navigation" },
    IconEntry { family: "Material", name: "check_box", codepoint: 0xe157, category: "Toggle" },
    IconEntry { family: "Material", name: "check_circle", codepoint: 0xe159, category: "Action" },
    IconEntry { family: "Material", name: "chevron_left", codepoint: 0xe15d, category: "Navigation" },
    IconEntry { family: "Material", name: "chevron_right", codepoint: 0xe15e, category: "Navigation" },
    IconEntry { family: "Material", name: "clear", codepoint: 0xe168, category: "Content" },
    IconEntry { family: "Material", name: "close", codepoint: 0xe16a, category: "Navigation" },
    IconEntry { family: "Material", name: "cloud", codepoint: 0xe16f, category: "File" },
    IconEntry { family: "Material", name: "code", codepoint: 0xe179, category: "Action" },
    IconEntry { family: "Material", name: "content_copy", codepoint: 0xe190, category: "Content" },
    IconEntry { family: "Material", name: "create", codepoint: 0xe1a6, category: "Content" },
    IconEntry { family: "Material", name: "dark_mode", codepoint: 0xe1ae, category: "Device" },
    IconEntry { family: "Material", name: "dashboard", codepoint: 0xe1b1, category: "Action" },
    IconEntry { family: "Material", name: "delete", codepoint: 0xe1b9, category: "Action" },
    IconEntry { family: "Material", name: "delete_outline", codepoint: 0xe1bb, category: "Action" },
    IconEntry { family: "Material", name: "description", codepoint: 0xe1c1, category: "Action" },
    IconEntry { family: "Material", name: "done", codepoint: 0xe1f6, category: "Action" },
    IconEntry { family: "Material", name: "download", codepoint: 0xe201, category: "File" },
    IconEntry { family: "Material", name: "edit", codepoint: 0xe21a, category: "Image" },
    IconEntry { family: "Material", name: "email", codepoint: 0xe22a, category: "Communication" },
    IconEntry { family: "Material", name: "error", codepoint: 0xe237, category: "Alert" },
    IconEntry { family: "Material", name: "error_outline", codepoint: 0xe238, category: "Alert" },
    IconEntry { family: "Material", name: "event", codepoint: 0xe23e, category: "Action" },
    IconEntry { family: "Material", name: "exit_to_app", codepoint: 0xe243, category: "Action" },
    IconEntry { family: "Material", name: "expand_less", codepoint: 0xe244, category: "Navigation" },
    IconEntry { family: "Material", name: "expand_more", codepoint: 0xe246, category: "Navigation" },
    IconEntry { family: "Material", name: "favorite", codepoint: 0xe25b, category: "Action" },
    IconEntry { family: "Material", name: "favorite_border", codepoint: 0xe25c, category: "Action" },
    IconEntry { family: "Material", name: "file_copy", codepoint: 0xe267, category: "Content" },
    IconEntry { family: "Material", name: "filter_list", codepoint: 0xe275, category: "Content" },
    IconEntry { family: "Material", name: "flutter_dash", codepoint: 0xe297, category: "Social" },
    IconEntry { family: "Material", name: "folder", codepoint: 0xe2a3, category: "File" },
    IconEntry { family: "Material", name: "help", codepoint: 0xe30b, category: "Action" },
    IconEntry { family: "Material", name: "help_outline", codepoint: 0xe30c, category: "Action" },
    IconEntry { family: "Material", name: "home", codepoint: 0xe318, category: "Action" },
    IconEntry { family: "Material", name: "image", codepoint: 0xe332, category: "Image" },
    IconEntry { family: "Material", name: "info", codepoint: 0xe33c, category: "Action" },
    IconEntry { family: "Material", name: "info_outline", codepoint: 0xe33d, category: "Action" },
    IconEntry { family: "Material", name: "keyboard_arrow_down", codepoint: 0xe353, category: "Hardware" },
    IconEntry { family: "Material", name: "keyboard_arrow_right", codepoint: 0xe355, category: "Hardware" },
    IconEntry { family: "Material", name: "language", codepoint: 0xe366, category: "Action" },
    IconEntry { family: "Material", name: "light_mode", codepoint: 0xe37b, category: "Device" },
    IconEntry { family: "Material", name: "link", codepoint: 0xe37f, category: "Content" },
    IconEntry { family: "Material", name: "list", codepoint: 0xe384, category: "Action" },
    IconEntry { family: "Material", name: "location_on", codepoint: 0xe3ab, category: "Communication" },
    IconEntry { family: "Material", name: "lock", codepoint: 0xe3ae, category: "Action" },
    IconEntry { family: "Material", name: "login", codepoint: 0xe3b2, category: "Action" },
    IconEntry { family: "Material", name: "logout", codepoint: 0xe3b3, category: "Action" },
    IconEntry { family: "Material", name: "mail", codepoint: 0xe3c3, category: "Content" },
    IconEntry { family: "Material", name: "menu", codepoint: 0xe3dc, category: "Navigation" },
    IconEntry { family: "Material", name: "more_horiz", codepoint: 0xe402, category: "Navigation" },
    IconEntry { family: "Material", name: "more_vert", codepoint: 0xe404, category: "Navigation" },
    IconEntry { family: "Material", name: "notifications", codepoint: 0xe44f, category: "Social" },
    IconEntry { family: "Material", name: "open_in_new", codepoint: 0xe462, category: "Action" },
    IconEntry { family: "Material", name: "palette", codepoint: 0xe46d, category: "Image" },
    IconEntry { family: "Material", name: "pause", codepoint: 0xe476, category: "AV" },
    IconEntry { family: "Material", name: "person", codepoint: 0xe491, category: "Social" },
    IconEntry { family: "Material", name: "phone", codepoint: 0xe4a2, category: "Communication" },
    IconEntry { family: "Material", name: "photo", codepoint: 0xe4a9, category: "Image" },
    IconEntry { family: "Material", name: "play_arrow", codepoint: 0xe4cb, category: "AV" },
    IconEntry { family: "Material", name: "refresh", codepoint: 0xe514, category: "Navigation" },
    IconEntry { family: "Material", name: "remove", codepoint: 0xe516, category: "Content" },
    IconEntry { family: "Material", name: "save", codepoint: 0xe550, category: "Content" },
    IconEntry { family: "Material", name: "schedule", codepoint: 0xe556, category: "Action" },
    IconEntry { family: "Material", name: "search", codepoint: 0xe567, category: "Action" },
    IconEntry { family: "Material", name: "send", codepoint: 0xe571, category: "Content" },
    IconEntry { family: "Material", name: "settings", codepoint: 0xe57f, category: "Action" },
    IconEntry { family: "Material", name: "share", codepoint: 0xe593, category: "Social" },
    IconEntry { family: "Material", name: "shopping_cart", codepoint: 0xe59c, category: "Action" },
    IconEntry { family: "Material", name: "star", codepoint: 0xe5f9, category: "Toggle" },
    IconEntry { family: "Material", name: "star_border", codepoint: 0xe5fa, category: "Toggle" },
    IconEntry { family: "Material", name: "stop", codepoint: 0xe604, category: "AV" },
    IconEntry { family: "Material", name: "sync", codepoint: 0xe627, category: "Notification" },
    IconEntry { family: "Material", name: "thumb_up", codepoint: 0xe65b, category: "Action" },
    IconEntry { family: "Material", name: "timer", codepoint: 0xe662, category: "Image" },
    IconEntry { family: "Material", name: "tune", codepoint: 0xe688, category: "Image" },
    IconEntry { family: "Material", name: "upload", codepoint: 0xe69e, category: "File" },
    IconEntry { family: "Material", name: "visibility", codepoint: 0xe6b5, category: "Action" },
    IconEntry { family: "Material", name: "visibility_off", codepoint: 0xe6b6, category: "Action" },
    IconEntry { family: "Material", name: "warning", codepoint: 0xe6cb, category: "Alert" },
    IconEntry { family: "Material", name: "widgets", codepoint: 0xe6e1, category: "Device" },
    IconEntry { family: "Cupertino", name: "add", codepoint: 0xf489, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "back", codepoint: 0xf3cf, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "bell", codepoint: 0xf3e1, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "bookmark", codepoint: 0xf3e7, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "check_mark", codepoint: 0xf3fd, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "chevron_back", codepoint: 0xf5c8, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "chevron_forward", codepoint: 0xf5ca, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "clear", codepoint: 0xf404, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "delete", codepoint: 0xf4c4, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "folder", codepoint: 0xf434, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "gear", codepoint: 0xf43d, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "heart", codepoint: 0xf442, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "home", codepoint: 0xf447, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "info", codepoint: 0xf44c, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "person", codepoint: 0xf47d, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "plus", codepoint: 0xf489, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "refresh", codepoint: 0xf49a, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "search", codepoint: 0xf4a4, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "settings", codepoint: 0xf43d, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "share", codepoint: 0xf4ca, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "star", codepoint: 0xf4b2, category: "Cupertino" },
    IconEntry { family: "Cupertino", name: "trash", codepoint: 0xf4c4, category: "Cupertino" },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_flutter_color_forms() {
        let src = r#"
final a = Color(0xFF1E88E5);
final b = Color.fromARGB(255, 10, 20, 30);
final c = Color.fromRGBO(255, 128, 0, 0.5);
final d = Colors.blue.shade700;
final e = Colors.red[500];
final f = CupertinoColors.activeBlue;
"#;
        let colors = find_flutter_colors(src);
        assert_eq!(colors.len(), 6);
        assert_eq!(colors[0].argb, 0xFF1E88E5);
        assert_eq!(colors[1].argb, 0xFF0A141E);
        assert_eq!((colors[2].argb >> 24) & 0xff, 128);
        assert_eq!(colors[3].argb, 0xFF1976D2);
        assert_eq!(colors[4].argb, 0xFFF44336);
        assert_eq!(colors[5].argb, 0xFF007AFF);
    }

    #[test]
    fn finds_material_and_cupertino_icons() {
        let src = "Icon(Icons.flutter_dash), Icon(CupertinoIcons.heart)";
        let icons = find_flutter_icons(src);
        assert_eq!(icons.len(), 2);
        assert_eq!(icons[0].entry.unwrap().codepoint, 0xe297);
        assert_eq!(icons[1].entry.unwrap().family, "Cupertino");
    }

    #[test]
    fn handles_multibyte_utf8_in_colors_and_icons() {
        let src = r#"
// Header — em-dash, café, rocket 🚀
const label = 'Status — ready';
final c1 = Color(0xFF1E88E5);//—comment
final c2 = Colors.blue;//—after
const i1 = Icon(Icons.add);//—after
const i2 = Icon(CupertinoIcons.heart);//é
"#;
        let colors = find_flutter_colors(src);
        assert_eq!(colors.len(), 2);
        assert_eq!(colors[0].argb, 0xFF1E88E5);
        assert_eq!(colors[1].label, "Colors.blue");

        let icons = find_flutter_icons(src);
        assert_eq!(icons.len(), 2);
        assert_eq!(icons[0].full_name, "Icons.add");
        assert_eq!(icons[1].full_name, "CupertinoIcons.heart");
    }
}
