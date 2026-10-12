//! Built-in post-install fix-ups for gallery skins. They only touch the user's
//! local copy of a skin; the original is kept next to it.

use crate::ini::{decode_text, detect, encode_text};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Names accepted in a catalog `PostInstall=` value.
pub const KNOWN: [&str; 1] = ["mond-weather-wttr"];

/// Run the named fix-up on the skin roots in `roots` (absolute paths).
/// Returns a one-line note on success.
pub fn run(name: &str, roots: &[PathBuf]) -> Result<String, String> {
    match name {
        "mond-weather-wttr" => {
            let mut patched = Vec::new();
            for r in roots {
                for f in find_files(r, "Weather.ini") {
                    if mond_weather_wttr(&f).map_err(|e| format!("{}: {e}", f.display()))? {
                        patched.push(f.display().to_string());
                    }
                }
            }
            if patched.is_empty() {
                Err("no Yahoo-based Weather.ini found to patch".into())
            } else {
                Ok(format!("mond-weather-wttr: patched {}", patched.join(", ")))
            }
        }
        other => Err(format!("unknown post-install fix-up \"{other}\"")),
    }
}

fn find_files(dir: &Path, name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            out.extend(find_files(&p, name));
        } else if ft.is_file() && e.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            out.push(p);
        }
    }
    out
}

const YAHOO_URL: &str = "https://www.yahoo.com/news/weather";
const WTTR_URL: &str = "URL=https://wttr.in/?format=j1";
const REGEXP: &str = "RegExp=(?siU)\"temp_F\":\\s*\"(.*)\".*\"weatherCode\":\\s*\"(.*)\".*\"weatherDesc\":\\s*\\[\\s*\\{\\s*\"value\":\\s*\"(.*)\"\
.*\"nearest_area\".*\"areaName\":\\s*\\[\\s*\\{\\s*\"value\":\\s*\"(.*)\"";
const ICONS: [&str; 2] = [
    "RegExpSubstitute=1",
    "Substitute=\"^113$\":\"clear_day\",\"^116$\":\"partly_cloudy_day\",\"^(119|122)$\":\"cloudy\",\
\"^(143|248|260)$\":\"fog\",\"^(179|182|185|281|284|311|314|317|320|350|362|365|374|377)$\":\"Sleet\",\
\"^(227|230|323|326|329|332|335|338|368|371|392|395)$\":\"snow\",\
\"^(176|200|263|266|293|296|299|302|305|308|353|356|359|386|389)$\":\"rain\"",
];
/// Yahoo's capture order was location, icon, condition, F; wttr's is F, code, condition, location.
const NEW_INDEX: [(&str, u32); 4] =
    [("MeasureLocation", 4), ("MeasureWeatherIcons", 2), ("MeasureWeatherCond", 3), ("MeasureTempFahrenheit", 1)];

/// Point Mond's `Weather.ini` at wttr.in (keyless JSON, location by IP) instead
/// of the dead Yahoo page. The file keeps its encoding; the first run saves the
/// original as `Weather.ini.yahoo-original` and every run patches from that
/// backup, so it is idempotent. Returns whether the file was changed.
pub fn mond_weather_wttr(ini: &Path) -> io::Result<bool> {
    let backup = ini.with_file_name("Weather.ini.yahoo-original");
    if !backup.exists() {
        fs::copy(ini, &backup)?;
    }
    let bytes = fs::read(&backup)?;
    let enc = detect(&bytes);
    let text = decode_text(&bytes);
    let mut out = String::with_capacity(text.len() + 1024);
    let mut section = String::new();
    let mut done_index: Vec<String> = Vec::new();
    let mut icons_done = false;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\r', '\n']);
        let eol = &line[body.len()..];
        let eol = if eol.is_empty() { "\r\n" } else { eol };
        let t = body.trim();
        if t.starts_with('[') && t.ends_with(']') {
            section = t[1..t.len() - 1].to_string();
            out.push_str(line);
            continue;
        }
        let replaced: Option<Vec<String>> = if t.strip_prefix("URL=").is_some_and(|v| v.trim() == YAHOO_URL) {
            Some(vec![WTTR_URL.into()])
        } else if body.starts_with("RegExp=(?siU)<h1") {
            Some(vec![REGEXP.into()])
        } else if body.starts_with("StringIndex=") && !done_index.contains(&section) {
            NEW_INDEX.iter().find(|(s, _)| *s == section).map(|(s, i)| {
                done_index.push(s.to_string());
                vec![format!("StringIndex={i}")]
            })
        } else if section == "MeasureWeatherIcons" && body.starts_with("Substitute=") && !icons_done {
            icons_done = true;
            Some(ICONS.iter().map(|s| s.to_string()).collect())
        } else {
            None
        };
        match replaced {
            Some(lines) => {
                for l in lines {
                    out.push_str(&l);
                    out.push_str(eol);
                }
            }
            None => out.push_str(line),
        }
    }
    if out == text {
        return Ok(false);
    }
    fs::write(ini, encode_text(&out, enc))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAHOO: &str = "[Rainmeter]\r\nUpdate=1000\r\n\r\n[MeasureWeather]\r\nMeasure=WebParser\r\nURL=https://www.yahoo.com/news/weather\r\n\
RegExp=(?siU)<h1 class=\"x\">(.*)</h1>.*\r\nUpdateRate=600\r\n\r\n\
[MeasureLocation]\r\nMeasure=WebParser\r\nURL=[MeasureWeather]\r\nStringIndex=1\r\n\r\n\
[MeasureWeatherIcons]\r\nMeasure=WebParser\r\nURL=[MeasureWeather]\r\nStringIndex=3\r\nSubstitute=\"a\":\"b\"\r\n\r\n\
[MeasureWeatherCond]\r\nMeasure=WebParser\r\nURL=[MeasureWeather]\r\nStringIndex=2\r\n\r\n\
[MeasureTempFahrenheit]\r\nMeasure=WebParser\r\nURL=[MeasureWeather]\r\nStringIndex=4\r\n\r\n[Other]\r\nStringIndex=9\r\n";

    fn utf16(s: &str) -> Vec<u8> {
        let mut v = vec![0xFF, 0xFE];
        for u in s.encode_utf16() {
            v.extend_from_slice(&u.to_le_bytes());
        }
        v
    }

    #[test]
    fn patches_idempotently_and_keeps_utf16() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("Weather.ini");
        fs::write(&p, utf16(YAHOO)).unwrap();
        assert!(mond_weather_wttr(&p).unwrap());
        let first = fs::read(&p).unwrap();
        assert_eq!(&first[..2], &[0xFF, 0xFE]);
        let text = decode_text(&first);
        assert!(text.contains("URL=https://wttr.in/?format=j1\r\n"));
        assert!(!text.contains("yahoo.com"));
        assert!(text.contains("\"temp_F\""));
        assert!(text.contains("RegExpSubstitute=1\r\nSubstitute=\"^113$\":\"clear_day\""));
        let ini = crate::ini::IniFile::parse(&text);
        assert_eq!(ini.get("MeasureLocation", "StringIndex"), Some("4"));
        assert_eq!(ini.get("MeasureWeatherIcons", "StringIndex"), Some("2"));
        assert_eq!(ini.get("MeasureWeatherCond", "StringIndex"), Some("3"));
        assert_eq!(ini.get("MeasureTempFahrenheit", "StringIndex"), Some("1"));
        assert_eq!(ini.get("Other", "StringIndex"), Some("9"));
        // Original saved, byte for byte.
        assert_eq!(fs::read(d.path().join("Weather.ini.yahoo-original")).unwrap(), utf16(YAHOO));
        // Second run: same bytes, backup untouched.
        assert!(mond_weather_wttr(&p).unwrap());
        assert_eq!(fs::read(&p).unwrap(), first);
        assert_eq!(fs::read(d.path().join("Weather.ini.yahoo-original")).unwrap(), utf16(YAHOO));
    }

    #[test]
    fn run_reports_missing_and_unknown() {
        let d = tempfile::tempdir().unwrap();
        assert!(run("mond-weather-wttr", &[d.path().to_path_buf()]).is_err());
        assert!(run("nope", &[]).is_err());
        fs::create_dir_all(d.path().join("Mond/Weather")).unwrap();
        fs::write(d.path().join("Mond/Weather/Weather.ini"), utf16(YAHOO)).unwrap();
        assert!(run("mond-weather-wttr", &[d.path().join("Mond")]).unwrap().contains("patched"));
    }
}
