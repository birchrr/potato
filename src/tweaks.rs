//! What Potato changes in Roblox's own files, as plain text edits that can be tested anywhere.
//!
//! Two files per Windows user:
//! - `GlobalBasicSettings_*.xml`: the in-game settings menu (graphics quality, maximum frame rate). Roblox
//!   rewrites it when it closes, so it's only written while that user's Roblox is closed.
//! - `ClientAppSettings.json`: Fast Flags. Roblox only reads the flags on its allowlist; the Ultra Low flags below are on it.

use serde_json::{Map, Value};

/// What Potato changes in one account's Roblox files.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub lowest_graphics: bool,
    /// None leaves the frame rate alone
    pub fps_cap: Option<u32>,
    /// lowest textures, no grass, no anti-aliasing: the game looks worse than the lowest menu setting
    pub ultra_low: bool,
    /// open every Roblox window maximized with its title bar: Roblox's own Fullscreen setting (which hides the
    /// title bar) goes off, and helper.rs maximizes each new window
    pub maximized: bool,
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

/// GlobalBasicSettings with the plan applied. Graphics: manual mode at the lowest level (the values Roblox
/// writes when you drag Graphics Quality all the way down). Frame rate: Roblox's own Maximum Frame Rate setting (-1 means its default, 60).
pub fn apply_basic_settings(xml: &str, plan: &Plan) -> String {
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
    if plan.maximized {
        set("Fullscreen", "false".into());
    }
    out
}

/// The Fast Flags Ultra Low turns on: every flag on Roblox's allowlist that lowers quality or work.
pub const ULTRA_LOW_FLAGS: [(&str, &str); 13] = [
    ("DFFlagTextureQualityOverrideEnabled", "True"),
    ("DFIntTextureQualityOverride", "0"),
    ("FIntFRMMaxGrassDistance", "0"),
    ("FIntFRMMinGrassDistance", "0"),
    ("FIntDebugForceMSAASamples", "0"),
    ("DFIntDebugFRMQualityLevelOverride", "1"),        // render quality below the menu's lowest
    ("DFFlagDisableDPIScale", "True"),                 // draws at a lower resolution on scaled screens
    ("DFIntCSGLevelOfDetailSwitchingDistance", "0"),   // built shapes always use their lowest-detail model
    ("DFIntCSGLevelOfDetailSwitchingDistanceL12", "0"),
    ("DFIntCSGLevelOfDetailSwitchingDistanceL23", "0"),
    ("DFIntCSGLevelOfDetailSwitchingDistanceL34", "0"),
    ("FFlagDebugSkyGray", "True"),
    ("DFFlagDebugPauseVoxelizer", "True"),             // stops lighting updates: less CPU, lighting can look frozen
];

/// ClientAppSettings.json with Potato's flags added (Ultra Low on) or taken out (off). Every other flag
/// the user or a launcher put there stays. A file that isn't a JSON object is left alone (None).
pub fn apply_flags(json: &str, plan: &Plan) -> Option<String> {
    let mut flags: Map<String, Value> = if json.trim().is_empty() { Map::new() } else { serde_json::from_str(json).ok()? };
    let on = plan.ultra_low;
    for (k, v) in ULTRA_LOW_FLAGS {
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
<bool name="Fullscreen">true</bool>
</Properties></Item></roblox>"#;

    fn plan(fps: Option<u32>, ultra_low: bool) -> Plan {
        Plan { lowest_graphics: true, fps_cap: fps, ultra_low, maximized: false }
    }

    #[test]
    fn maximized_turns_roblox_fullscreen_off() {
        let p = Plan { maximized: true, ..plan(None, false) };
        assert_eq!(xml_get(&apply_basic_settings(XML, &p), "Fullscreen"), Some("false"));
        assert_eq!(xml_get(&apply_basic_settings(XML, &plan(None, false)), "Fullscreen"), Some("true"));
    }

    #[test]
    fn sets_lowest_graphics_and_fps_and_nothing_else() {
        let out = apply_basic_settings(XML, &plan(Some(30), false));
        assert_eq!(xml_get(&out, "FramerateCap"), Some("30"));
        assert_eq!(xml_get(&out, "GraphicsQualityLevel"), Some("3"));
        assert_eq!(xml_get(&out, "SavedQualityLevel"), Some("1"));
        assert_eq!(xml_get(&out, "CameraYInverted"), Some("false"));
        assert_eq!(out.lines().zip(XML.lines()).filter(|(a, b)| a != b).count(), 3);
    }

    #[test]
    fn missing_settings_are_not_added() {
        let out = apply_basic_settings("<roblox></roblox>", &plan(Some(30), false));
        assert_eq!(out, "<roblox></roblox>");
        assert_eq!(xml_set("<roblox></roblox>", "FramerateCap", "30"), None);
    }

    #[test]
    fn leaving_fps_alone_keeps_the_users_cap() {
        let out = apply_basic_settings(XML, &plan(None, false));
        assert_eq!(xml_get(&out, "FramerateCap"), Some("-1"));
    }

    #[test]
    fn flags_keep_the_users_own_and_come_back_out() {
        let mine = r#"{"FFlagHandleAltEnterFullscreenManually": "False"}"#;
        let on = apply_flags(mine, &plan(None, true)).unwrap();
        let v: Value = serde_json::from_str(&on).unwrap();
        assert_eq!(v["DFIntTextureQualityOverride"], "0");
        assert_eq!(v["FFlagHandleAltEnterFullscreenManually"], "False");
        let off = apply_flags(&on, &plan(None, false)).unwrap();   // turning Ultra Low off takes them out again
        let v: Value = serde_json::from_str(&off).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 1);
    }

    #[test]
    fn flags_the_user_set_differently_are_not_removed() {
        let mine = r#"{"FIntDebugForceMSAASamples": "4"}"#;
        let v: Value = serde_json::from_str(&apply_flags(mine, &plan(None, false)).unwrap()).unwrap();
        assert_eq!(v["FIntDebugForceMSAASamples"], "4");
    }

    #[test]
    fn broken_flag_files_are_left_alone() {
        assert_eq!(apply_flags("not json", &plan(None, true)), None);
        assert!(apply_flags("", &plan(None, true)).is_some());
    }
}

/// CPU cores for each client when every client gets `per` cores of its own: client i takes the next `per`
/// cores, wrapping around when there are more clients than cores. Bit n of a mask is logical core n.
pub fn core_masks(clients: usize, cores: usize, per: usize) -> Vec<u64> {
    let cores = cores.clamp(1, 64);
    let per = per.clamp(1, cores);
    (0..clients)
        .map(|i| (0..per).fold(0u64, |m, k| m | 1 << ((i * per + k) % cores)))
        .collect()
}

#[cfg(test)]
mod core_tests {
    use super::core_masks;

    #[test]
    fn each_client_gets_its_own_cores() {
        assert_eq!(core_masks(3, 8, 2), vec![0b11, 0b1100, 0b110000]);
    }

    #[test]
    fn wraps_when_clients_outnumber_cores() {
        assert_eq!(core_masks(5, 4, 1), vec![1, 2, 4, 8, 1]);
        assert_eq!(core_masks(2, 3, 2), vec![0b011, 0b101]);
    }

    #[test]
    fn never_asks_for_more_cores_than_exist() {
        assert_eq!(core_masks(1, 4, 9), vec![0b1111]);
        assert_eq!(core_masks(1, 128, 1), vec![1]);
    }
}
