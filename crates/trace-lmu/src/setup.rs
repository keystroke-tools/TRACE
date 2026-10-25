//! Bounded parsing of LMU's text-based `.svm` setup identity header.

/// Simulator identities declared by an LMU setup file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LmuSetupIdentity {
    pub vehicle_class: String,
    pub vehicle_package: Option<String>,
}

/// Reads the stable identity header without interpreting adjustable setup values.
///
/// # Errors
///
/// Returns an error for non-text data, a missing/unsafe vehicle class, or an
/// inconsistent vehicle package path.
pub fn parse_setup_identity(contents: &[u8]) -> Result<LmuSetupIdentity, &'static str> {
    let text = std::str::from_utf8(contents).map_err(|_| "LMU setup is not UTF-8 text")?;
    let mut vehicle_class = None;
    let mut vehicle_package = None;
    for line in text.lines().take(64).map(str::trim) {
        if let Some(value) = line.strip_prefix("VehicleClassSetting=") {
            vehicle_class = quoted_value(value).map(str::to_owned);
        } else if let Some(path) = line.strip_prefix("//VEH=") {
            vehicle_package = vehicle_package_from_path(path);
        }
    }
    let vehicle_class = vehicle_class
        .filter(|value| safe_identity(value))
        .ok_or("LMU setup does not declare a safe VehicleClassSetting")?;
    if vehicle_package
        .as_deref()
        .is_some_and(|value| !safe_identity(value))
    {
        return Err("LMU setup declares an unsafe vehicle package");
    }
    Ok(LmuSetupIdentity {
        vehicle_class,
        vehicle_package,
    })
}

fn quoted_value(value: &str) -> Option<&str> {
    value
        .trim()
        .strip_prefix('"')?
        .strip_suffix('"')
        .filter(|value| !value.is_empty())
}

fn vehicle_package_from_path(path: &str) -> Option<String> {
    let components = path.split(['\\', '/']).collect::<Vec<_>>();
    components
        .windows(2)
        .find(|pair| pair[0].eq_ignore_ascii_case("Vehicles"))
        .map(|pair| pair[1].to_owned())
        .filter(|value| !value.is_empty())
}

fn safe_identity(value: &str) -> bool {
    value.len() <= 160
        && !value.trim().is_empty()
        && value
            .chars()
            .all(|character| !character.is_control() && !matches!(character, '<' | '>' | '"' | '|'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vehicle_class_and_package_from_realistic_header() {
        let identity = parse_setup_identity(
            br#"VehicleClassSetting="ELMS2025 LMP3 Ligier_JS_P325"
UpgradeSetting=(42497,0,0,0)
//VEH=C:\Program Files (x86)\Steam\steamapps\common\Le Mans Ultimate\Installed\Vehicles\Ligier_JSP325_2025\1.09\example.VEH
[GENERAL]
FuelSetting=50//55.0L
"#,
        )
        .expect("valid setup");
        assert_eq!(identity.vehicle_class, "ELMS2025 LMP3 Ligier_JS_P325");
        assert_eq!(
            identity.vehicle_package.as_deref(),
            Some("Ligier_JSP325_2025")
        );
    }

    #[test]
    fn rejects_missing_or_binary_identity() {
        assert!(parse_setup_identity(b"[GENERAL]\nFuelSetting=1").is_err());
        assert!(parse_setup_identity(&[0xff, 0xfe]).is_err());
    }
}
