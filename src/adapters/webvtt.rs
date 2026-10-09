//! WebVTT transcripts as meeting tools export them: Teams names each speaker
//! with a `<v Name>` span, Zoom writes `Name: text` in the cue.

/// One cue: when it was said, by whom when the file says, and what.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub speaker: Option<String>,
    pub text: String,
}

/// Whether `head`, a file's first bytes, opens a WebVTT file.
pub fn is_webvtt(head: &[u8]) -> bool {
    let head = head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head);
    head.strip_prefix(b"WEBVTT").is_some_and(|rest| {
        rest.first()
            .is_none_or(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r'))
    })
}

/// The cues of a WebVTT file, in the order they start.
pub fn parse(text: &str) -> Result<Vec<Cue>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    if !is_webvtt(text.as_bytes()) {
        return Err("not a WebVTT file".into());
    }
    let mut cues = Vec::new();
    // Blocks are separated by blank lines; the first is the header.
    for block in text.split("\n\n").skip(1) {
        let lines: Vec<&str> = block.lines().filter(|line| !line.is_empty()).collect();
        let Some(timing) = lines.iter().position(|line| line.contains("-->")) else {
            continue; // SESSION, STYLE and REGION blocks, or stray text
        };
        let (start, end) = timings(lines[timing])?;
        let payload = lines[timing + 1..].join("\n");
        let speaker = voice(&payload);
        let text = words(&strip_tags(&payload));
        if !text.is_empty() {
            cues.push(Cue {
                start,
                end,
                speaker,
                text,
            });
        }
    }
    if cues.is_empty() {
        return Err("no cues".into());
    }
    name_zoom_speakers(&mut cues);
    cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(cues)
}

/// Cues for transcript lines `(start, speaker, text)` in order. Lines carry no
/// end: a cue lasts as its words take to say, at most until the next line.
pub fn cues(lines: &[(f64, String, String)]) -> Vec<Cue> {
    lines
        .iter()
        .enumerate()
        .map(|(i, (start, speaker, text))| {
            let spoken = (text.split_whitespace().count() as f64 * 0.35).clamp(1.0, 10.0);
            let next = lines
                .get(i + 1)
                .map(|line| line.0)
                .filter(|next| next > start);
            let end = next.map_or(start + spoken, |next| next.min(start + spoken));
            // WebVTT counts milliseconds.
            let millis = |seconds: f64| (seconds * 1000.0).round() / 1000.0;
            Cue {
                start: millis(*start),
                end: millis(end),
                speaker: Some(speaker.clone()),
                text: text.clone(),
            }
        })
        .collect()
}

/// A transcript as WebVTT: one cue per line, its speaker in a voice span.
pub fn write(cues: &[Cue]) -> String {
    let mut text = String::from("WEBVTT\n");
    for cue in cues {
        let said = escape(&cue.text);
        let payload = match &cue.speaker {
            Some(speaker) => format!("<v {}>{said}</v>", escape(speaker)),
            None => said,
        };
        text.push_str(&format!(
            "\n{} --> {}\n{payload}\n",
            stamp(cue.start),
            stamp(cue.end)
        ));
    }
    text
}

/// Seconds as `hh:mm:ss.ttt`.
fn stamp(seconds: f64) -> String {
    let millis = (seconds.max(0.0) * 1000.0).round() as u64;
    let (hours, rest) = (millis / 3_600_000, millis % 3_600_000);
    format!(
        "{hours:02}:{:02}:{:02}.{:03}",
        rest / 60_000,
        rest / 1000 % 60,
        rest % 1000
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `00:01:02.500 --> 00:01:04.000 align:start` as seconds.
fn timings(line: &str) -> Result<(f64, f64), String> {
    let (start, rest) = line.split_once("-->").expect("callers found the arrow");
    let end = rest.split_whitespace().next().unwrap_or_default();
    Ok((timestamp(start.trim())?, timestamp(end)?))
}

/// `hh:mm:ss.ttt`, the hours optional.
fn timestamp(text: &str) -> Result<f64, String> {
    let bad = || format!("bad timestamp {text:?}");
    let (clock, millis) = text.split_once('.').ok_or_else(bad)?;
    let parts: Vec<&str> = clock.split(':').collect();
    if !(2..=3).contains(&parts.len()) || millis.len() != 3 {
        return Err(bad());
    }
    let number = |part: &str| part.parse::<u64>().map_err(|_| bad());
    let seconds = parts
        .iter()
        .try_fold(0, |total, part| Ok::<_, String>(total * 60 + number(part)?))?;
    Ok(seconds as f64 + number(millis)? as f64 / 1000.0)
}

/// The name in the payload's first `<v Name>` span.
fn voice(payload: &str) -> Option<String> {
    let start = payload.find("<v")?;
    let tag = &payload[start + 2..start + payload[start..].find('>')?];
    // `<v.loud Name>`: classes come before the whitespace, the name after it.
    let name = tag.split_once(char::is_whitespace)?.1;
    Some(decode(name.trim())).filter(|name| !name.is_empty())
}

/// The text without markup: every `<…>` is a tag in WebVTT, since a literal `<`
/// must be written `&lt;`.
fn strip_tags(payload: &str) -> String {
    let mut text = String::with_capacity(payload.len());
    let mut inside = false;
    for c in payload.chars() {
        match c {
            '<' => inside = true,
            '>' if inside => inside = false,
            c if !inside => text.push(c),
            _ => {}
        }
    }
    decode(&text)
}

/// Lines and runs of spaces joined into single spaces: cues wrap mid-sentence.
fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Character references: the named ones WebVTT defines and numeric ones
/// (`&#193;`, `&#xC1;`), which Teams uses for accented names.
fn decode(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        rest = &rest[start..];
        let reference = rest
            .find(';')
            .filter(|end| *end <= 10)
            .and_then(|end| character(&rest[1..end]).map(|c| (c, end)));
        match reference {
            Some((c, end)) => {
                decoded.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

fn character(name: &str) -> Option<char> {
    let code = match name {
        "amp" => '&' as u32,
        "lt" => '<' as u32,
        "gt" => '>' as u32,
        "quot" => '"' as u32,
        "apos" => '\'' as u32,
        "nbsp" => 0xa0,
        "lrm" => 0x200e,
        "rlm" => 0x200f,
        _ => match name.strip_prefix('#')? {
            hex if hex.starts_with(['x', 'X']) => u32::from_str_radix(&hex[1..], 16).ok()?,
            decimal => decimal.parse().ok()?,
        },
    };
    char::from_u32(code)
}

/// Zoom writes `Name: text` instead of voice spans. When no cue names its
/// speaker and nearly all start that way, the prefix is the speaker.
fn name_zoom_speakers(cues: &mut [Cue]) {
    let prefix = |text: &str| {
        let (name, said) = text.split_once(": ")?;
        let plausible = !name.is_empty() && name.chars().count() <= 40 && !said.is_empty();
        plausible.then(|| (name.to_string(), said.to_string()))
    };
    let named = cues.iter().filter(|c| prefix(&c.text).is_some()).count();
    if cues.iter().any(|c| c.speaker.is_some()) || named * 10 < cues.len() * 8 {
        return;
    }
    for cue in cues {
        if let Some((name, said)) = prefix(&cue.text) {
            cue.speaker = Some(name);
            cue.text = said;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(start: f64, end: f64, speaker: Option<&str>, text: &str) -> Cue {
        Cue {
            start,
            end,
            speaker: speaker.map(String::from),
            text: text.into(),
        }
    }

    /// As Teams writes it: CRLF, uuid cue ids, numeric references in names, one
    /// closed voice span per cue wrapping over lines, overlapping backchannels.
    const TEAMS: &str = "WEBVTT\r\n\r\n\
        3f6d2a10-1111-4abc-9def-000000000001/9-0\r\n\
        00:00:03.675 --> 00:00:07.381\r\n\
        <v &#193;lvaro Vin&#237;cius>Aqui no front,\r\n\
        isso aqui &#233; o quanto ele consegue,</v>\r\n\r\n\
        3f6d2a10-1111-4abc-9def-000000000001/7-0\r\n\
        00:00:07.275 --> 00:00:07.635\r\n\
        <v Bruna Costa>Mhm.</v>\r\n\r\n\
        3f6d2a10-1111-4abc-9def-000000000001/9-1\r\n\
        00:00:07.381 --> 00:00:10.293\r\n\
        <v &#193;lvaro Vin&#237;cius>tudo calculado no front &amp; no back.</v>\r\n";

    #[test]
    fn reads_teams_transcripts() {
        assert_eq!(
            parse(TEAMS).unwrap(),
            [
                cue(
                    3.675,
                    7.381,
                    Some("Álvaro Vinícius"),
                    "Aqui no front, isso aqui é o quanto ele consegue,"
                ),
                cue(7.275, 7.635, Some("Bruna Costa"), "Mhm."),
                cue(
                    7.381,
                    10.293,
                    Some("Álvaro Vinícius"),
                    "tudo calculado no front & no back."
                ),
            ]
        );
    }

    #[test]
    fn reads_zoom_transcripts_and_plain_captions() {
        let zoom = "WEBVTT\n\n1\n00:00:01.000 --> 00:00:03.000\nAna Lima: Bom dia.\n\n\
                    2\n00:00:03.500 --> 00:00:05.000\nCarlos: Vamos começar:\nprimeiro item.\n";
        assert_eq!(
            parse(zoom).unwrap(),
            [
                cue(1.0, 3.0, Some("Ana Lima"), "Bom dia."),
                cue(3.5, 5.0, Some("Carlos"), "Vamos começar: primeiro item."),
            ]
        );
        // Captions with an occasional colon are not speakers.
        let plain = "WEBVTT\n\n00:01.000 --> 00:02.000\nNota: isto não é um nome.\n\n\
                     00:02.000 --> 00:03.000\nUma legenda.\n\n00:03.000 --> 00:04.000\nOutra.\n";
        let cues = parse(plain).unwrap();
        assert!(cues.iter().all(|c| c.speaker.is_none()));
        assert_eq!(cues[0].text, "Nota: isto não é um nome.");
    }

    #[test]
    fn follows_the_rest_of_the_format() {
        let text = "\u{feff}WEBVTT - exported\n\nSESSION made by hand\nover lines\n\n\
                    STYLE\n::cue { color: red }\n\n\
                    01:00:00.000 --> 01:00:02.500 align:start position:10%\n\
                    <v.loud Eve>Hi <c.yellow>there</c> <00:00:01.000>&lt;3&gt;\n\n\
                    00:00:00.500 --> 00:00:01.000\n<i>   </i>\n";
        assert_eq!(
            parse(text).unwrap(),
            [cue(3600.0, 3602.5, Some("Eve"), "Hi there <3>")]
        );
        assert!(
            is_webvtt(b"WEBVTT\n") && is_webvtt(b"\xef\xbb\xbfWEBVTT") && !is_webvtt(b"WEBVTTX")
        );
    }

    #[test]
    fn written_transcripts_read_back_the_same() {
        let lines = vec![
            (
                3.675,
                "Álvaro Vinícius".to_string(),
                "Front & back <ok>".to_string(),
            ),
            (3.675, "Bruna".to_string(), "Mhm.".to_string()),
            (
                7.381,
                "Álvaro Vinícius".to_string(),
                "um dois três quatro cinco seis sete oito nove dez".to_string(),
            ),
            (3725.004, "Bruna".to_string(), "Fim.".to_string()),
        ];
        let written = cues(&lines);
        // Until the next line, or as long as the words take; never past ten seconds.
        let ends: Vec<f64> = written.iter().map(|c| c.end).collect();
        assert_eq!(ends, [5.075, 4.675, 10.881, 3726.004]);
        let text = write(&written);
        assert!(text.starts_with("WEBVTT\n\n00:00:03.675 --> 00:00:05.075\n<v Álvaro Vinícius>Front &amp; back &lt;ok&gt;</v>\n"));
        assert!(text.contains("01:02:05.004 --> 01:02:06.004"));
        assert_eq!(parse(&text).unwrap(), written);
    }

    #[test]
    fn rejects_what_is_not_webvtt() {
        assert!(parse("1\n00:00:01,000 --> 00:00:02,000\nSRT\n").is_err());
        assert!(parse("WEBVTT\n\n00:00:01,000 --> 00:00:02,000\nComma\n").is_err());
        assert!(parse("WEBVTT\n\nSESSION nothing else\n").is_err());
        assert_eq!(
            decode("&#xC1;&bogus; & &#99999999;"),
            "Á&bogus; & &#99999999;"
        );
    }
}
