use super::{Budget, Compiled, Request, Truth};
use crate::feed_sets::MAX_SETS_PER_MATCHER;
use crate::geoip::{GeoDatabase, GeoDatabases, Lookup, MAX_LIST_ENTRIES};
use crate::policy::schema::AsnMatch;
use crate::{AsnSet, PolicySets};
use anyhow::{Context, Result, ensure};
use std::sync::Arc;

pub(super) fn compile_country(
  codes: &[String],
  path: &str,
  depth: usize,
  budget: &mut Budget,
  geoip: &GeoDatabases,
) -> Result<Compiled> {
  budget.consume(depth, path)?;
  let database = geoip
    .country()
    .with_context(|| format!("{path}.country: requires signals.geoip.country"))?;
  ensure!(
    (1..=MAX_LIST_ENTRIES).contains(&codes.len()),
    "{path}.country: expected 1 to {MAX_LIST_ENTRIES} country codes"
  );
  let codes = codes
    .iter()
    .enumerate()
    .map(|(index, code)| {
      <[u8; 2]>::try_from(code.as_bytes())
        .ok()
        .filter(|code| code.iter().all(u8::is_ascii_uppercase))
        .with_context(|| format!("{path}.country[{index}]: expected an ISO 3166-1 alpha-2 code"))
    })
    .collect::<Result<Vec<_>>>()?;
  Ok(Compiled::Country(database.clone(), codes))
}

pub(super) fn compile_asn(
  value: &AsnMatch,
  path: &str,
  depth: usize,
  budget: &mut Budget,
  sets: &PolicySets,
  geoip: &GeoDatabases,
) -> Result<Compiled> {
  budget.consume(depth, path)?;
  let database = geoip
    .asn()
    .with_context(|| format!("{path}.asn: requires signals.geoip.asn"))?;
  match value {
    AsnMatch::Numbers(numbers) => {
      ensure!(
        (1..=MAX_LIST_ENTRIES).contains(&numbers.len()),
        "{path}.asn: expected 1 to {MAX_LIST_ENTRIES} autonomous system numbers"
      );
      Ok(Compiled::Asn(database.clone(), numbers.clone()))
    }
    AsnMatch::Sets(names) => {
      ensure!(
        (1..=MAX_SETS_PER_MATCHER).contains(&names.len()),
        "{path}.asn.in_sets: in_sets requires 1 to 16 set names"
      );
      let resolved = names
        .iter()
        .map(|name| {
          sets
            .asn
            .get(name)
            .cloned()
            .with_context(|| format!("{path}.asn.in_sets: unknown asn set {name}"))
        })
        .collect::<Result<Vec<_>>>()?;
      Ok(Compiled::AsnSets(database.clone(), resolved))
    }
  }
}

pub(super) fn match_country(
  database: &GeoDatabase,
  codes: &[[u8; 2]],
  request: &Request<'_>,
) -> Truth {
  let Some(ip) = request.client_ip else {
    return Truth::False;
  };
  match database.country(ip) {
    Lookup::Unknown => Truth::Unknown,
    Lookup::Known(code) => Truth::from_bool(code.is_some_and(|code| codes.contains(&code))),
  }
}

pub(super) fn match_asn(database: &GeoDatabase, numbers: &[u32], request: &Request<'_>) -> Truth {
  let Some(ip) = request.client_ip else {
    return Truth::False;
  };
  match database.asn(ip) {
    Lookup::Unknown => Truth::Unknown,
    Lookup::Known(number) => {
      Truth::from_bool(number.is_some_and(|number| numbers.contains(&number)))
    }
  }
}

pub(super) fn match_asn_sets(
  database: &GeoDatabase,
  sets: &[Arc<AsnSet>],
  request: &Request<'_>,
) -> Truth {
  let Some(ip) = request.client_ip else {
    return Truth::False;
  };
  match database.asn(ip) {
    Lookup::Unknown => Truth::Unknown,
    Lookup::Known(None) => Truth::False,
    Lookup::Known(Some(number)) => Truth::any(sets.iter().map(|set| set.membership(number).into())),
  }
}
