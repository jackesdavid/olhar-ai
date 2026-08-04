//! The transactional messages themselves (card #165) — welcome, password reset, password changed.
//!
//! Each returns a ready [`Email`] with **both** a text and an html part. Text is not a fallback
//! afterthought: it renders in every client, and html-only mail is penalised by spam filters —
//! which for a password reset means the customer never gets back into their account.
//!
//! ## Carrying the identity into a medium that fights you
//!
//! The brand is the **dot-field**: a hexagon of circles, largest and darkest at the centre,
//! shrinking and dimming toward the edge — awareness condensing to a focal point. Email cannot use
//! the `SentinelMark` component, and **images are blocked by default in most clients**, so a logo
//! would arrive as a broken box. The mark is therefore built from HTML: seven round `<div>`s in
//! hexagonal packing with radial falloff. In clients that ignore `border-radius` (Outlook's Word
//! engine) it degrades to seven small squares with the same falloff — still calm, still the motif,
//! never broken.
//!
//! Everything else follows the ink theme: soft-gray canvas, white card lifting off it, one
//! near-black ink, **no brand hue**. The only colour is `--destructive` red on the "this was not
//! me" path, which survives because it communicates STATE, not brand.
//!
//! No external stylesheet, no `<style>` block, no web fonts, no images. Every rule is inline.
use super::email::Email;

/// The product name **as customers see it**. The codebase, services and repo stay "Sentinel";
/// only the copy is Olhar AI (card #76). One constant, so the two never drift apart in a string
/// literal someone forgot.
const PRODUCT: &str = "Olhar AI";

/// Ink theme, resolved to literals because email has no custom properties.
const INK: &str = "#18181b"; // --primary / --foreground (near-black)
const INK_SOFT: &str = "#3f3f46"; // body copy
const INK_MUTED: &str = "#71717a"; // --muted-foreground
const CANVAS: &str = "#f6f6f7"; // page behind the card
const CARD_BORDER: &str = "#e6e6e8";
const DANGER: &str = "#b42318"; // --destructive, state only

/// The dot-field mark: one bright focal dot with a ring of six, in hexagonal packing.
///
/// Radial falloff is the hierarchy rule of the whole system, so it is literal here — the centre is
/// 12px at full ink, the ring 7px at 40%. Rows are 2/3/2, which is what a hex ring looks like when
/// you flatten it into lines an email client can position without absolute coordinates.
fn dot_field() -> String {
    let dot = |size: u32, opacity: &str, margin: u32| {
        format!(
            r#"<span style="display:inline-block;width:{size}px;height:{size}px;margin:0 {margin}px;background:{INK};opacity:{opacity};border-radius:50%;"></span>"#
        )
    };
    let ring = |n: usize| (0..n).map(|_| dot(7, "0.40", 4)).collect::<String>();
    format!(
        r#"    <div style="text-align:center;line-height:1;margin:0 0 28px;">
      <div style="margin-bottom:5px;">{}</div>
      <div style="margin-bottom:5px;">{}{}{}</div>
      <div>{}</div>
    </div>"#,
        ring(2),
        dot(7, "0.40", 4),
        dot(12, "1", 4),
        dot(7, "0.40", 4),
        ring(2),
    )
}

/// The shared shell: dot-field mark, eyebrow, heading, body, footer — centred, calm, generous.
///
/// `preheader` is the grey line clients show beside the subject in the inbox list. Without one they
/// show the first words of the body, which is the greeting and tells the reader nothing.
fn shell(preheader: &str, eyebrow: &str, heading: &str, body: &str) -> String {
    format!(
        r#"<div style="margin:0;padding:32px 12px;background:{CANVAS};">
  <span style="display:none!important;visibility:hidden;opacity:0;height:0;width:0;overflow:hidden;">{preheader}</span>
  <div style="max-width:560px;margin:0 auto;background:#ffffff;border:1px solid {CARD_BORDER};border-radius:14px;padding:40px 36px;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:{INK};font-size:15px;line-height:1.6;">
{}
    <div style="font-size:11px;font-weight:600;text-transform:uppercase;letter-spacing:0.08em;color:{INK_MUTED};margin:0 0 10px;">{eyebrow}</div>
    <div style="font-size:21px;font-weight:600;letter-spacing:-0.01em;line-height:1.3;margin:0 0 20px;">{heading}</div>
{body}
  </div>
  <div style="max-width:560px;margin:20px auto 0;text-align:center;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;font-size:12px;line-height:1.5;color:{INK_MUTED};">
    {PRODUCT} — camera monitoring and automation
  </div>
</div>"#,
        dot_field()
    )
}

/// The primary action. A real `<a>` styled as a button — a `<button>` does nothing in mail — and
/// the destination is ALWAYS printed as a plain link too, because some clients strip the styling
/// and some readers simply will not click a button.
fn button(href: &str, label: &str) -> String {
    format!(
        r#"    <div style="margin:28px 0 20px;">
      <a href="{href}" style="display:inline-block;background:{INK};color:#ffffff;text-decoration:none;padding:13px 24px;border-radius:10px;font-weight:600;font-size:15px;line-height:1;">{label}</a>
    </div>
    <div style="font-size:13px;color:{INK_MUTED};margin:0 0 6px;">If the button does not work, paste this into your browser:</div>
    <div style="font-size:13px;word-break:break-all;margin:0 0 24px;"><a href="{href}" style="color:{INK};">{href}</a></div>"#
    )
}

/// Sent once, right after signup. Its job is the FIRST ACTION, not a greeting: a monitoring product
/// is worth nothing until a camera is attached, and this is the moment of highest intent.
pub fn welcome(to: &str, name: &str, app_url: &str) -> Email {
    let name = display_name(name, to);
    let text = format!(
        "Welcome to {PRODUCT}, {name}.\n\n\
         Your account is ready and your trial has started.\n\n\
         To see anything useful, connect your first camera:\n\
         1. Install the appliance on a machine that stays on at home or at the site.\n\
         2. Pair it with the code the app shows you.\n\
         3. Add a camera — {PRODUCT} finds most of them on the network by itself.\n\n\
         Start here: {app_url}\n\n\
         If you get stuck, reply to this email.\n"
    );
    let html = shell(
        "Connect your first camera to finish setting up.",
        "Welcome",
        &format!("You're in, {name}."),
        &format!(
            r#"    <div style="margin:0 0 18px;color:{INK_SOFT};">Your account is ready and your trial has started. To see anything useful, connect your first camera:</div>
    <div style="margin:0 0 6px;color:{INK_SOFT};"><strong style="color:{INK};">1.</strong> Install the appliance on a machine that stays on at home or at the site.</div>
    <div style="margin:0 0 6px;color:{INK_SOFT};"><strong style="color:{INK};">2.</strong> Pair it with the code the app shows you.</div>
    <div style="margin:0;color:{INK_SOFT};"><strong style="color:{INK};">3.</strong> Add a camera — {PRODUCT} finds most of them on the network by itself.</div>
{}
    <div style="font-size:13px;color:{INK_MUTED};">If you get stuck, just reply to this email.</div>"#,
            button(app_url, "Set up my first camera")
        ),
    );
    Email::new(to, format!("Welcome to {PRODUCT}"), text).with_html(html)
}

/// The one that decides whether a locked-out customer comes back or churns. States the expiry
/// plainly — a reset link that has quietly expired is indistinguishable from one that never worked.
pub fn password_reset(to: &str, name: &str, reset_url: &str, valid_minutes: u32) -> Email {
    let name = display_name(name, to);
    let text = format!(
        "Hello {name},\n\n\
         Someone asked to reset the password for your {PRODUCT} account ({to}).\n\n\
         Set a new password here — the link works for {valid_minutes} minutes:\n\
         {reset_url}\n\n\
         If this was not you, you can ignore this email. Your password stays as it is, \
         and nobody can use this link without opening it from your inbox.\n"
    );
    let html = shell(
        &format!("Reset your password — the link is valid for {valid_minutes} minutes."),
        "Account",
        "Reset your password",
        &format!(
            r#"    <div style="margin:0 0 14px;color:{INK_SOFT};">Hello {name} — someone asked to reset the password for <strong style="color:{INK};">{to}</strong>.</div>
    <div style="margin:0;color:{INK_SOFT};">The link below works for <strong style="color:{INK};">{valid_minutes} minutes</strong>.</div>
{}
    <div style="font-size:13px;color:{INK_MUTED};">If this was not you, ignore this email — your password stays as it is, and nobody can use this link without opening it from your inbox.</div>"#,
            button(reset_url, "Set a new password")
        ),
    );
    Email::new(to, format!("Reset your {PRODUCT} password"), text).with_html(html)
}

/// Sent AFTER a password changes, to the address on the account. This exists for exactly one
/// reader: the person whose account was just taken. Everyone else deletes it, which is fine — the
/// mail costs nothing and not sending it costs a silent account takeover.
pub fn password_changed(to: &str, name: &str, when: &str, support_url: &str) -> Email {
    let name = display_name(name, to);
    let text = format!(
        "Hello {name},\n\n\
         The password for your {PRODUCT} account ({to}) was changed on {when}.\n\n\
         If you made this change, there is nothing to do.\n\n\
         If you did not change it, act now — someone else may have access to your account \
         and your cameras:\n\
         {support_url}\n"
    );
    let html = shell(
        "Your password was changed. If this was not you, act now.",
        "Security",
        "Your password was changed",
        &format!(
            r#"    <div style="margin:0 0 14px;color:{INK_SOFT};">Hello {name} — the password for <strong style="color:{INK};">{to}</strong> was changed on <strong style="color:{INK};">{when}</strong>.</div>
    <div style="margin:0 0 18px;color:{INK_SOFT};">If you made this change, there is nothing to do.</div>
    <div style="border-left:3px solid {DANGER};padding:2px 0 2px 14px;margin:0;color:{INK_SOFT};">If you did <strong style="color:{DANGER};">not</strong> change it, act now — someone else may have access to your account and your cameras.</div>
{}"#,
            button(support_url, "Secure my account")
        ),
    );
    Email::new(to, format!("Your {PRODUCT} password was changed"), text).with_html(html)
}

/// Address the reader by name, falling back to the local part of their email. Never "Hello ,".
fn display_name(name: &str, email: &str) -> String {
    let n = name.trim();
    if !n.is_empty() {
        return n.to_string();
    }
    email
        .split('@')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("there")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<Email> {
        vec![
            welcome("jo@example.com", "Jo", "https://app.olharai.com"),
            password_reset("jo@example.com", "Jo", "https://app.olharai.com/reset?t=abc", 30),
            password_changed(
                "jo@example.com",
                "Jo",
                "3 August 2026 at 21:04",
                "https://app.olharai.com/help",
            ),
        ]
    }

    /// Every message carries BOTH parts. An html-only transactional mail is the one that lands in
    /// spam, and for a password reset that is a customer who cannot get back in.
    #[test]
    fn every_template_has_a_text_and_an_html_part() {
        for e in all() {
            assert!(!e.subject.trim().is_empty(), "subject empty");
            assert!(e.text.len() > 40, "text part too thin: {:?}", e.text);
            assert!(e.html.expect("html part").contains("<div"));
        }
    }

    /// Email blocks images and drops `<style>`. Anything external is a broken box in the reader's
    /// client, so the rule is enforced rather than remembered.
    #[test]
    fn no_external_assets_and_no_style_block() {
        for e in all() {
            let html = e.html.unwrap_or_default();
            for banned in ["<img", "src=", "<style", "<link", "background-image", "@font-face"] {
                assert!(!html.contains(banned), "{banned:?} must not appear in email html");
            }
        }
    }

    /// The identity has to survive a medium with no components: the dot-field is drawn in HTML,
    /// with the centre dot larger and fully inked and the ring dimmed — radial falloff, literally.
    #[test]
    fn the_dot_field_mark_is_present_with_radial_falloff() {
        for e in all() {
            let html = e.html.unwrap_or_default();
            assert!(html.contains("border-radius:50%"), "dots must be round");
            assert!(html.contains("width:12px"), "missing the bright focal dot");
            assert!(html.contains("opacity:0.40"), "missing the dimmed outer ring");
            assert_eq!(
                html.matches("border-radius:50%").count(),
                7,
                "the mark is one centre dot plus a ring of six"
            );
        }
    }

    /// No brand hue on surfaces or the primary action — the button is ink, per the theme.
    #[test]
    fn the_primary_action_is_ink_not_a_brand_colour() {
        let e = welcome("jo@example.com", "Jo", "https://x");
        let html = e.html.unwrap();
        assert!(html.contains(&format!("background:{INK};color:#ffffff")));
        for hue in ["#10b981", "emerald", "#22c55e"] {
            assert!(!html.contains(hue), "retired brand hue {hue} reintroduced");
        }
    }

    /// Colour survives only where it means STATE. Red appears on the takeover path and nowhere else.
    #[test]
    fn danger_colour_is_used_only_for_the_security_path() {
        assert!(welcome("j@e.com", "Jo", "https://x").html.unwrap().find(DANGER).is_none());
        assert!(password_reset("j@e.com", "Jo", "https://x", 30).html.unwrap().find(DANGER).is_none());
        assert!(password_changed("j@e.com", "Jo", "now", "https://x").html.unwrap().contains(DANGER));
    }

    /// A template that ships an unsubstituted placeholder tells the customer the product is broken
    /// at the exact moment they needed it to work.
    #[test]
    fn no_unsubstituted_placeholders_survive() {
        for e in all() {
            let html = e.html.unwrap_or_default();
            for body in [&e.text, &html] {
                for marker in ["{}", "{{", "}}", "{name}", "{to}"] {
                    assert!(!body.contains(marker), "placeholder {marker:?} left in: {body}");
                }
            }
        }
    }

    /// The link is the payload. It must appear in the text part too — that is what renders when the
    /// html is stripped.
    #[test]
    fn the_action_link_appears_in_both_parts() {
        let url = "https://app.olharai.com/reset?t=abc";
        let e = password_reset("jo@example.com", "Jo", url, 30);
        assert!(e.text.contains(url), "reset link missing from the text part");
        assert!(e.html.unwrap().contains(url), "reset link missing from html");
    }

    /// A silently-expired link is indistinguishable from a broken one.
    #[test]
    fn the_reset_states_its_expiry() {
        let e = password_reset("jo@example.com", "Jo", "https://x/y", 30);
        assert!(e.text.contains("30 minutes"));
        assert!(e.html.unwrap().contains("30 minutes"));
    }

    /// The whole reason this message exists.
    #[test]
    fn password_changed_tells_the_reader_what_to_do_if_it_was_not_them() {
        let e = password_changed("jo@example.com", "", "today", "https://x/help");
        assert!(e.text.to_lowercase().contains("did not change it"));
        assert!(e.html.unwrap().contains("https://x/help"));
    }

    /// Never "Hello ,".
    #[test]
    fn a_missing_name_falls_back_to_the_email_local_part() {
        let e = welcome("jo@example.com", "   ", "https://x");
        assert!(e.text.contains("jo"), "expected the local part: {}", e.text);
    }
}
