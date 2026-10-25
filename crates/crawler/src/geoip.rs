use std::net::IpAddr;
use std::path::Path;

use ckbadger_store::{Asn, Geo};

/// Resolves an IP to its geographic location and autonomous system.
///
/// A per-IP miss is honest `None`; database read/decode errors carry context.
pub trait GeoIp: Send + Sync {
    fn lookup(&self, ip: IpAddr) -> anyhow::Result<(Option<Geo>, Option<Asn>)>;
}

/// Used when no MMDB is configured: everything is honestly Unknown.
pub struct NoGeo;

impl GeoIp for NoGeo {
    fn lookup(&self, _ip: IpAddr) -> anyhow::Result<(Option<Geo>, Option<Asn>)> {
        Ok((None, None))
    }
}

/// MaxMind GeoLite2 backed resolver (City + ASN databases).
pub struct MaxmindGeoIp {
    city: maxminddb::Reader<Vec<u8>>,
    asn: maxminddb::Reader<Vec<u8>>,
}

impl MaxmindGeoIp {
    /// Fail-fast: returns `Err` if either MMDB path is missing/unreadable/corrupt.
    pub fn open(city_path: &Path, asn_path: &Path) -> anyhow::Result<Self> {
        let city = maxminddb::Reader::open_readfile(city_path)
            .map_err(|e| anyhow::anyhow!("open GeoLite2 City '{}': {e}", city_path.display()))?;
        let asn = maxminddb::Reader::open_readfile(asn_path)
            .map_err(|e| anyhow::anyhow!("open GeoLite2 ASN '{}': {e}", asn_path.display()))?;
        Ok(Self { city, asn })
    }
}

impl GeoIp for MaxmindGeoIp {
    fn lookup(&self, ip: IpAddr) -> anyhow::Result<(Option<Geo>, Option<Asn>)> {
        let geo = lookup_record(self.city.lookup::<maxminddb::geoip2::City>(ip), ip, "City")?
            .and_then(city_record_to_geo);
        let asn = lookup_record(self.asn.lookup::<maxminddb::geoip2::Asn>(ip), ip, "ASN")?
            .and_then(asn_record_to_asn);
        Ok((geo, asn))
    }
}

fn lookup_record<T>(
    result: Result<T, maxminddb::MaxMindDBError>,
    ip: IpAddr,
    database: &str,
) -> anyhow::Result<Option<T>> {
    match result {
        Ok(record) => Ok(Some(record)),
        Err(maxminddb::MaxMindDBError::AddressNotFoundError(_)) => Ok(None),
        Err(error) => anyhow::bail!("GeoIP {database} lookup failed for ip={ip}: {error}"),
    }
}

fn city_record_to_geo(record: maxminddb::geoip2::City<'_>) -> Option<Geo> {
    let country = record.country?.iso_code?.to_string();
    let city = record
        .city
        .and_then(|value| value.names)
        .and_then(|names| names.get("en").copied())
        .unwrap_or_default()
        .to_string();
    let lat = record.location.as_ref().and_then(|value| value.latitude);
    let lon = record.location.as_ref().and_then(|value| value.longitude);
    Some(Geo {
        country,
        city,
        lat,
        lon,
    })
}

fn asn_record_to_asn(record: maxminddb::geoip2::Asn<'_>) -> Option<Asn> {
    let number = record.autonomous_system_number.filter(|value| *value > 0)?;
    Some(Asn {
        number,
        org: record
            .autonomous_system_organization
            .unwrap_or_default()
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;
    use std::path::Path;

    #[test]
    fn nogeo_returns_none() {
        let g = NoGeo;
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        assert_eq!(g.lookup(ip).unwrap(), (None, None));
    }

    #[test]
    fn maxmind_open_fails_fast_on_bad_path() {
        // Path configured but unreadable => Err (fail-fast), never a silent NoGeo fallback.
        let err = MaxmindGeoIp::open(
            Path::new("/no/such/city.mmdb"),
            Path::new("/no/such/asn.mmdb"),
        );
        assert!(err.is_err());
    }

    #[test]
    fn missing_coordinates_and_asn_number_remain_absent() {
        let city: maxminddb::geoip2::City<'_> =
            serde_json::from_str(r#"{"country":{"iso_code":"US"},"location":{"latitude":40.7}}"#)
                .unwrap();
        let geo = city_record_to_geo(city).unwrap();
        assert_eq!(geo.country, "US");
        assert_eq!(geo.lat, Some(40.7));
        assert_eq!(geo.lon, None);
        let asn: maxminddb::geoip2::Asn<'_> =
            serde_json::from_str(r#"{"autonomous_system_organization":"Unknown provider"}"#)
                .unwrap();
        assert_eq!(asn_record_to_asn(asn), None);
    }

    #[test]
    fn address_miss_is_unknown_but_decode_error_is_reported() {
        use maxminddb::MaxMindDBError;
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        assert_eq!(
            lookup_record::<u8>(
                Err(MaxMindDBError::AddressNotFoundError("miss".into())),
                ip,
                "City"
            )
            .unwrap(),
            None
        );
        let error = lookup_record::<u8>(
            Err(MaxMindDBError::DecodingError("bad record".into())),
            ip,
            "City",
        )
        .unwrap_err();
        assert!(error.to_string().contains("1.2.3.4"));
        assert!(error.to_string().contains("bad record"));
    }
}
