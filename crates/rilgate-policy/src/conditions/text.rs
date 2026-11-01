use super::{Budget, Compiled, Request, Truth};
use crate::policy::schema::{Condition, SameAs, StringMatch};
use anyhow::{Context, Result, ensure};
use http::HeaderName;
use regex::Regex;

#[derive(Debug)]
pub(crate) enum Field {
  Path,
  Host,
  Method,
  UserAgent,
  Header(HeaderName),
  EdgeHeader(HeaderName),
}
#[derive(Debug)]
pub(crate) struct Text {
  source: StringMatch,
  regex: Option<Regex>,
}

/// Compiles the request field matchers of one condition into `nodes`.
pub(super) fn compile_fields(
  condition: &Condition,
  path: &str,
  depth: usize,
  budget: &mut Budget,
  nodes: &mut Vec<Compiled>,
) -> Result<()> {
  for (field, label, source) in [
    (Field::Path, "path", &condition.path),
    (Field::Host, "host", &condition.host),
    (Field::Method, "method", &condition.method),
    (Field::UserAgent, "user_agent", &condition.user_agent),
  ] {
    if let Some(source) = source {
      budget.consume(depth, path)?;
      let test = Text::compile(source, budget).with_context(|| format!("{path}.{label}"))?;
      nodes.push(Compiled::Text(field, test));
    }
  }
  for (label, headers, edge) in [
    ("headers", &condition.headers, false),
    ("edge_headers", &condition.edge_headers, true),
  ] {
    for (name, source) in headers {
      budget.consume(depth, path)?;
      let header = HeaderName::from_bytes(name.as_bytes())
        .with_context(|| format!("{path}.{label}: expected an HTTP header name"))?;
      let matcher_name = if edge { name.as_str() } else { header.as_str() };
      let test =
        Text::compile(source, budget).with_context(|| format!("{path}.{label}.{matcher_name}"))?;
      let field = if edge {
        Field::EdgeHeader(header)
      } else {
        Field::Header(header)
      };
      nodes.push(Compiled::Text(field, test));
    }
  }
  Ok(())
}

pub(super) fn match_field(field: &Field, test: &Text, request: &Request<'_>) -> Truth {
  let value = match field {
    Field::Path => request.path,
    Field::Host => request.host,
    Field::Method => request.method.as_str(),
    Field::UserAgent => request
      .headers
      .get(http::header::USER_AGENT)
      .and_then(|v| v.to_str().ok())
      .unwrap_or_default(),
    Field::EdgeHeader(_) if !request.trusted_peer => return Truth::False,
    Field::Header(name) | Field::EdgeHeader(name) => {
      return Truth::from_bool(
        request
          .headers
          .get_all(name)
          .iter()
          .any(|v| v.to_str().is_ok_and(|v| test.matches(v, request))),
      );
    }
  };
  Truth::from_bool(test.matches(value, request))
}

impl Text {
  fn compile(source: &StringMatch, budget: &mut Budget) -> Result<Self> {
    let count = [
      source.exact.is_some(),
      source.prefix.is_some(),
      source.contains.is_some(),
      source.values.is_some(),
      source.regex.is_some(),
      source.same_as.is_some(),
    ]
    .into_iter()
    .filter(|v| *v)
    .count();
    ensure!(
      count == 1,
      "string matcher requires exactly one of exact, prefix, contains, in, regex, same_as"
    );
    if let Some(values) = &source.values {
      ensure!(
        !values.is_empty() && values.len() <= 64,
        "in requires 1 to 64 values"
      );
    }
    for text in source
      .exact
      .iter()
      .chain(source.prefix.iter())
      .chain(source.contains.iter())
      .chain(source.regex.iter())
      .chain(source.values.iter().flatten())
    {
      ensure!(
        text.len() <= 2048,
        "matcher values must not exceed 2048 bytes"
      );
      budget.bytes += text.len();
    }
    ensure!(
      budget.bytes <= 65_536,
      "policy matcher values must not exceed 64 KiB in total"
    );
    let regex = source
      .regex
      .as_ref()
      .map(|v| {
        regex::RegexBuilder::new(v)
          .size_limit(262_144)
          .dfa_size_limit(262_144)
          .build()
      })
      .transpose()
      .context("regex: invalid or too complex regular expression")?;
    Ok(Self {
      source: source.clone(),
      regex,
    })
  }
  fn matches(&self, value: &str, request: &Request<'_>) -> bool {
    self.source.exact.as_ref().is_none_or(|v| value == v)
      && self
        .source
        .prefix
        .as_ref()
        .is_none_or(|v| value.starts_with(v))
      && self
        .source
        .contains
        .as_ref()
        .is_none_or(|v| value.contains(v))
      && self
        .source
        .values
        .as_ref()
        .is_none_or(|v| v.iter().any(|v| v == value))
      && self.regex.as_ref().is_none_or(|v| v.is_match(value))
      && self.source.same_as.is_none_or(|field| match field {
        SameAs::Host => {
          let host = request.host;
          let hostname = if host.starts_with('[') {
            host.find(']').map_or(host, |end| &host[..=end])
          } else {
            host
              .rsplit_once(':')
              .filter(|(_, port)| port.bytes().all(|byte| byte.is_ascii_digit()))
              .map_or(host, |(name, _)| name)
          };
          value.eq_ignore_ascii_case(hostname)
        }
      })
  }
}
