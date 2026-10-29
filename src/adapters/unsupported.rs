//! The seams a platform has no adapter for yet (docs/design.md §15): each
//! says so when used, and none fails silently.

use crate::ports::DesktopIntegration;

/// Why every use of this adapter fails: the platform has no adapter for the seam yet.
fn unsupported(what: &str) -> String {
    format!("{what} is not available on this system yet")
}

pub struct Unsupported;

impl DesktopIntegration for Unsupported {
    fn integrate(&self) -> Result<Option<String>, String> {
        Ok(Some(unsupported("starting eco from the desktop")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_says_eco_is_not_tied_into_the_desktop() {
        let said = Unsupported.integrate().unwrap().unwrap();
        assert!(
            said.contains("is not available on this system yet"),
            "{said}"
        );
    }
}
