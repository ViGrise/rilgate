use crate::geoip::{
  DEFAULT_MAX_STALE, DEFAULT_REFRESH, GeoDatabase, GeoKind, GeoSource, MAX_MAX_STALE, MAX_REFRESH,
  MIN_FILE_REFRESH, MIN_MAXMIND_REFRESH,
};
use crate::ip_sets::describe;
use crate::policy::schema::{GeoDatabaseDefinition, GeoIpDefinition, MaxMindSource};
use anyhow::{Result, ensure};
use std::sync::Arc;

/// The compiled GeoIP databases referenced by policy conditions.
#[derive(Debug, Default, Clone)]
pub struct GeoDatabases {
  country: Option<Arc<GeoDatabase>>,
  asn: Option<Arc<GeoDatabase>>,
}

impl GeoDatabases {
  pub fn compile(definition: Option<&GeoIpDefinition>) -> Result<Self> {
    Self::compile_with(definition, None)
  }

  /// Compiles the definition. A database whose source matches the one in
  /// `previous` keeps the loaded data of that database.
  pub fn compile_with(
    definition: Option<&GeoIpDefinition>,
    previous: Option<&GeoDatabases>,
  ) -> Result<Self> {
    let Some(definition) = definition else {
      return Ok(Self::default());
    };
    let build = |kind: GeoKind,
                 source: Option<&GeoDatabaseDefinition>,
                 earlier: Option<&Arc<GeoDatabase>>| {
      source
        .map(|source| {
          let database = compile_database(kind, source)?;
          Ok::<_, anyhow::Error>(Arc::new(match earlier {
            Some(earlier) => database.adopt_state_of(earlier),
            None => database,
          }))
        })
        .transpose()
    };
    Ok(Self {
      country: build(
        GeoKind::Country,
        definition.country.as_ref(),
        previous.and_then(GeoDatabases::country),
      )?,
      asn: build(
        GeoKind::Asn,
        definition.asn.as_ref(),
        previous.and_then(GeoDatabases::asn),
      )?,
    })
  }

  pub fn country(&self) -> Option<&Arc<GeoDatabase>> {
    self.country.as_ref()
  }

  pub fn asn(&self) -> Option<&Arc<GeoDatabase>> {
    self.asn.as_ref()
  }

  pub fn iter(&self) -> impl Iterator<Item = &Arc<GeoDatabase>> {
    self.country.iter().chain(self.asn.iter())
  }

  pub fn all_loaded(&self) -> bool {
    self.iter().all(|database| database.status().loaded)
  }
}

fn compile_database(kind: GeoKind, definition: &GeoDatabaseDefinition) -> Result<GeoDatabase> {
  let path = format!("policy.geoip.{}", kind.label());
  ensure!(
    definition.path.is_some() ^ definition.maxmind.is_some(),
    "{path}: expected exactly one of path, maxmind"
  );
  let (source, minimum) = match (&definition.path, &definition.maxmind) {
    (Some(file), _) => {
      ensure!(
        !file.as_os_str().is_empty(),
        "{path}.path: expected a file path"
      );
      (GeoSource::File(file.clone()), MIN_FILE_REFRESH)
    }
    (_, Some(maxmind)) => {
      check_maxmind(&path, maxmind)?;
      (GeoSource::MaxMind(maxmind.clone()), MIN_MAXMIND_REFRESH)
    }
    _ => unreachable!("one source is present"),
  };
  let refresh = definition.refresh.unwrap_or(DEFAULT_REFRESH);
  ensure!(
    (minimum..=MAX_REFRESH).contains(&refresh),
    "{path}.refresh: expected {} to {}",
    describe(minimum),
    describe(MAX_REFRESH)
  );
  let max_stale = definition.max_stale.unwrap_or(DEFAULT_MAX_STALE);
  ensure!(
    (refresh..=MAX_MAX_STALE).contains(&max_stale),
    "{path}.max_stale: expected refresh to {}",
    describe(MAX_MAX_STALE)
  );
  Ok(GeoDatabase::new(kind, source, refresh, max_stale))
}

fn check_maxmind(path: &str, source: &MaxMindSource) -> Result<()> {
  ensure!(
    (1..=64).contains(&source.edition.len())
      && source
        .edition
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
    "{path}.maxmind.edition: expected 1 to 64 bytes of A-Z, a-z, 0-9 and -"
  );
  ensure!(
    (1..=64).contains(&source.account_id.len())
      && source.account_id.bytes().all(|b| b.is_ascii_alphanumeric()),
    "{path}.maxmind.account_id: expected 1 to 64 bytes of A-Z, a-z and 0-9"
  );
  ensure!(
    !source.license_key.expose().is_empty(),
    "{path}.maxmind.license_key: required"
  );
  Ok(())
}
