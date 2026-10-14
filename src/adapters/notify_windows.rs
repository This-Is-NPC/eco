//! Windows: notifications as toasts, through the WinRT toast notifier of the
//! `windows` crate's safe projection. eco is not a packaged app and registers
//! no app id of its own, so its toasts go out under Windows PowerShell's,
//! which every Windows has: they work from the zip install, and show as
//! PowerShell's (docs/design.md §15).

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::core::HSTRING;

use crate::ports::{Notifier, Urgency};

/// Windows PowerShell's app id, which Windows always has registered.
const APP_ID: &str =
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

pub struct Toasts;

/// `text` safe inside an XML element or attribute.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            c => escaped.push(c),
        }
    }
    escaped
}

/// The toast's XML: `title` and `body`; a critical one stays on screen longer.
fn content(title: &str, body: &str, urgency: Urgency) -> String {
    let duration = match urgency {
        Urgency::Normal => "short",
        Urgency::Critical => "long",
    };
    format!(
        "<toast duration=\"{duration}\"><visual><binding template=\"ToastGeneric\">\
         <text>{}</text><text>{}</text></binding></visual></toast>",
        escape(title),
        escape(body)
    )
}

/// Show the toast `xml`.
fn show(xml: &str) -> windows::core::Result<()> {
    let document = XmlDocument::new()?;
    document.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&document)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&toast)
}

impl Notifier for Toasts {
    fn notify(&self, title: &str, body: &str, urgency: Urgency) {
        let xml = content(title, body, urgency);
        // On a thread of its own: no caller waits on Windows.
        std::thread::spawn(move || {
            if let Err(error) = show(&xml) {
                eprintln!("eco: the notification could not show: {error}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toast_carries_its_title_and_body_escaped_and_its_urgency_as_duration() {
        assert_eq!(
            content("Eles", "a < b & \"c\"", Urgency::Critical),
            "<toast duration=\"long\"><visual><binding template=\"ToastGeneric\">\
             <text>Eles</text><text>a &lt; b &amp; &quot;c&quot;</text></binding></visual></toast>"
        );
        assert!(content("eco", "ready", Urgency::Normal).starts_with("<toast duration=\"short\">"));
    }

    #[test]
    fn a_toast_s_xml_is_one_windows_reads() {
        let document = XmlDocument::new().unwrap();
        document
            .LoadXml(&HSTRING::from(content(
                "eco",
                "it's <fine>",
                Urgency::Normal,
            )))
            .unwrap();
        ToastNotification::CreateToastNotification(&document).unwrap();
    }
}
