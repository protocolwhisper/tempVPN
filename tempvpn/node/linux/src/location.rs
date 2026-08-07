const ISO_3166_ALPHA2: &str = "AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS YE YT ZA ZM ZW";

pub fn normalize_country_code(value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let normalized = value.trim().to_ascii_uppercase();
    if normalized.is_empty() {
        return Err("country_code cannot be empty when supplied".into());
    }
    if !ISO_3166_ALPHA2
        .split_ascii_whitespace()
        .any(|code| code == normalized)
    {
        return Err(format!(
            "country_code must be an ISO 3166-1 alpha-2 code, got {normalized}"
        ));
    }
    Ok(Some(normalized))
}

pub fn normalize_optional_text(field: &str, value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(format!("{field} cannot be empty when supplied"));
    }
    Ok(Some(normalized.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_valid_country_codes() {
        assert_eq!(
            normalize_country_code(Some(" de ")).unwrap().as_deref(),
            Some("DE")
        );
        assert_eq!(normalize_country_code(None).unwrap(), None);
    }

    #[test]
    fn rejects_empty_and_unsupported_country_codes() {
        assert!(normalize_country_code(Some(" ")).is_err());
        assert!(normalize_country_code(Some("ZZ")).is_err());
        assert!(normalize_country_code(Some("Germany")).is_err());
    }

    #[test]
    fn normalizes_optional_location_text() {
        assert_eq!(
            normalize_optional_text("city", Some(" Frankfurt "))
                .unwrap()
                .as_deref(),
            Some("Frankfurt")
        );
        assert_eq!(normalize_optional_text("city", None).unwrap(), None);
        assert!(normalize_optional_text("city", Some("")).is_err());
    }
}
