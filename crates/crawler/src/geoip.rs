use std::net::IpAddr;
use std::path::Path;

use ckbadger_store::{Asn, Geo};

/// Resolves an IP to its geographic location and autonomous system.
///
/// A per-IP miss is honest `None`, never a guessed default or a propagated
/// error — only [`MaxmindGeoIp::open`] fail-fasts (on missing/corrupt files).
pub trait GeoIp: Send + Sync {
    fn lookup(&self, ip: IpAddr) -> (Option<Geo>, Option<Asn>);
}

/// Used when no MMDB is configured: everything is honestly Unknown.
pub struct NoGeo;

impl GeoIp for NoGeo {
    fn lookup(&self, _ip: IpAddr) -> (Option<Geo>, Option<Asn>) {
        (None, None)
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
    fn lookup(&self, ip: IpAddr) -> (Option<Geo>, Option<Asn>) {
        // maxminddb 0.24 `lookup::<T>` returns `Result<T, _>`; a miss is
        // `Err(AddressNotFoundError)`. `.ok()` maps that miss (and any other
        // per-IP lookup error) to an honest `None` — the trait has no error
        // channel, and the databases were already validated in `open()`.
        // Subfields that are absent default to empty string / 0; the `Option`
        // is `Some` only when the lookup itself hit a record.
        let geo = self
            .city
            .lookup::<maxminddb::geoip2::City>(ip)
            .ok()
            .map(|c| Geo {
                country: c
                    .country
                    .and_then(|x| x.iso_code)
                    .unwrap_or_default()
                    .to_string(),
                city: c
                    .city
                    .and_then(|x| x.names)
                    .and_then(|n| n.get("en").copied())
                    .unwrap_or_default()
                    .to_string(),
                lat: c
                    .location
                    .as_ref()
                    .and_then(|l| l.latitude)
                    .unwrap_or_default(),
                lon: c
                    .location
                    .as_ref()
                    .and_then(|l| l.longitude)
                    .unwrap_or_default(),
            });
        let asn = self
            .asn
            .lookup::<maxminddb::geoip2::Asn>(ip)
            .ok()
            .map(|a| Asn {
                number: a.autonomous_system_number.unwrap_or_default(),
                org: a
                    .autonomous_system_organization
                    .unwrap_or_default()
                    .to_string(),
            });
        (geo, asn)
    }
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
        assert_eq!(g.lookup(ip), (None, None));
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
}
