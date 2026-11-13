//! What Potato changes in Roblox's own files, as plain text edits that can be tested anywhere.
//!
//! Two files per Windows user:
//! - `GlobalBasicSettings_*.xml`: the in-game settings menu (graphics quality, maximum frame rate). Roblox
//!   rewrites it when it closes, so it's only written while that user's Roblox is closed.
//! - `ClientAppSettings.json`: Fast Flags. Roblox only reads the flags on its allowlist; the extras below are on it.

use serde_json::{Map, Value};

/// Natro Macro warns below this frame rate, so Macro Safe mode never goes lower.
pub const MACRO_SAFE_MIN_FPS: u32 = 25;

/// What the user picked in the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub lowest_graphics: bool,
    /// None leaves the frame rate alone
    pub fps_cap: Option<u32>,
    /// lower textures, no grass, no anti-aliasing: these change how the game looks, so screen-reading macros can break
    pub extras: bool,
    pub macro_safe: bool,
}

impl Plan {
    /// The plan as it will be applied: Macro Safe drops the extras and keeps the frame rate at or above the minimum.
    pub fn effective(&self) -> Plan {
        if !self.macro_safe {
            return self.clone();
        }
        Plan { extras: false, fps_cap: self.fps_cap.map(|f| f.max(MACRO_SAFE_MIN_FPS)), ..self.clone() }
    }
}

/// The value inside `<anytag name="NAME">value</anytag>`, if the setting is in the file.
pub fn xml_get<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let (start, end) = value_span(xml, name)?;
    Some(&xml[start..end])
}

/// The file with one setting changed, or None when the setting isn't in the file (Potato never adds settings
/// Roblox didn't write itself).
pub fn xml_set(xml: &str, name: &str, value: &str) -> Option<String> {
    let (start, end) = value_span(xml, name)?;
    Some(format!("{}{}{}", &xml[..start], value, &xml[end..]))
}

fn value_span(xml: &str, name: &str) -> Option<(usize, usize)> {
    let key = format!("name=\"{name}\">");
    let start = xml.find(&key)? + key.len();
    let end = start + xml[start..].find('<')?;
    Some((start, end))
}

/// GlobalBasicSettings with the plan applied. Graphics: manual mode at the lowest level, the exact values
/// Natro Macro checks for. Frame rate: Roblox's own Maximum Frame Rate setting (-1 means its default, 60).
pub fn apply_basic_settings(xml: &str, plan: &Plan) -> String {
    let plan = plan.effective();
    let mut out = xml.to_string();
    let mut set = |name: &str, value: String| {
        if let Some(next) = xml_set(&out, name, &value) {
            out = next;
        }
    };
    if plan.lowest_graphics {
        set("SavedQualityLevel", "1".into());
        set("GraphicsQualityLevel", "3".into());
    }
    if let Some(fps) = plan.fps_cap {
        set("FramerateCap", fps.to_string());
    }
    out
}

/// The Fast Flags the extras turn on.
pub const EXTRA_FLAGS: [(&str, &str); 5] = [
    ("DFFlagTextureQualityOverrideEnabled", "True"),
    ("DFIntTextureQualityOverride", "0"),
    ("FIntFRMMaxGrassDistance", "0"),
    ("FIntFRMMinGrassDistance", "0"),
    ("FIntDebugForceMSAASamples", "0"),
];

/// ClientAppSettings.json with Potato's flags added (extras on) or taken out (extras off). Every other flag
/// the user or a launcher put there stays. A file that isn't a JSON object is left alone (None).
pub fn apply_flags(json: &str, plan: &Plan) -> Option<String> {
    let mut flags: Map<String, Value> = if json.trim().is_empty() { Map::new() } else { serde_json::from_str(json).ok()? };
    let on = plan.effective().extras;
    for (k, v) in EXTRA_FLAGS {
        if on {
            flags.insert(k.into(), Value::String(v.into()));
        } else if flags.get(k) == Some(&Value::String(v.into())) {
            flags.remove(k);
        }
    }
    Some(serde_json::to_string_pretty(&Value::Object(flags)).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<roblox><Item class="UserGameSettings"><Properties>
<int name="FramerateCap">-1</int>
<int name="GraphicsQualityLevel">0</int>
<token name="SavedQualityLevel">0</token>
<bool name="CameraYInverted">false</bool>
</Properties></Item></roblox>"#;

    fn plan(fps: Option<u32>, extras: bool, safe: bool) -> Plan {
        Plan { lowest_graphics: true, fps_cap: fps, extras, macro_safe: safe }
    }

    #[test]
    fn sets_lowest_graphics_and_fps_and_nothing_else() {
        let out = apply_basic_settings(XML, &plan(Some(30), false, true));
        assert_eq!(xml_get(&out, "FramerateCap"), Some("30"));
        assert_eq!(xml_get(&out, "GraphicsQualityLevel"), Some("3"));
        assert_eq!(xml_get(&out, "SavedQualityLevel"), Some("1"));
        assert_eq!(xml_get(&out, "CameraYInverted"), Some("false"));
        assert_eq!(out.lines().zip(XML.lines()).filter(|(a, b)| a != b).count(), 3);
    }

    #[test]
    fn macro_safe_keeps_fps_at_25_or_more_and_drops_extras() {
        let p = plan(Some(10), true, true).effective();
        assert_eq!(p.fps_cap, Some(MACRO_SAFE_MIN_FPS));
        assert!(!p.extras);
        let p = plan(Some(10), true, false).effective();
        assert_eq!(p.fps_cap, Some(10));
        assert!(p.extras);
    }

    #[test]
    fn missing_settings_are_not_added() {
        let out = apply_basic_settings("<roblox></roblox>", &plan(Some(30), false, true));
        assert_eq!(out, "<roblox></roblox>");
        assert_eq!(xml_set("<roblox></roblox>", "FramerateCap", "30"), None);
    }

    #[test]
    fn leaving_fps_alone_keeps_the_users_cap() {
        let out = apply_basic_settings(XML, &plan(None, false, true));
        assert_eq!(xml_get(&out, "FramerateCap"), Some("-1"));
    }

    #[test]
    fn flags_keep_the_users_own_and_come_back_out() {
        let mine = r#"{"FFlagHandleAltEnterFullscreenManually": "False"}"#;
        let on = apply_flags(mine, &plan(None, true, false)).unwrap();
        let v: Value = serde_json::from_str(&on).unwrap();
        assert_eq!(v["DFIntTextureQualityOverride"], "0");
        assert_eq!(v["FFlagHandleAltEnterFullscreenManually"], "False");
        let off = apply_flags(&on, &plan(None, true, true)).unwrap();   // Macro Safe turns them off again
        let v: Value = serde_json::from_str(&off).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 1);
    }

    #[test]
    fn flags_the_user_set_differently_are_not_removed() {
        let mine = r#"{"FIntDebugForceMSAASamples": "4"}"#;
        let v: Value = serde_json::from_str(&apply_flags(mine, &plan(None, false, true)).unwrap()).unwrap();
        assert_eq!(v["FIntDebugForceMSAASamples"], "4");
    }

    #[test]
    fn broken_flag_files_are_left_alone() {
        assert_eq!(apply_flags("not json", &plan(None, true, false)), None);
        assert!(apply_flags("", &plan(None, true, false)).is_some());
    }
}
