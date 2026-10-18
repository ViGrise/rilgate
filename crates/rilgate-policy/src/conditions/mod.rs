use crate::ip_sets::IpRanges;
use crate::policy::schema::{Condition, NumberMatch, StringMatch};
use anyhow::{Context, Result, ensure};
use http::{HeaderMap, HeaderName, Method};
use regex::Regex;
use std::net::IpAddr;

pub struct Request<'a> {
  pub headers: &'a HeaderMap,
  pub path: &'a str,
  pub method: &'a Method,
  pub client_ip: Option<IpAddr>,
}

#[derive(Debug)]
pub(crate) enum Compiled {
  All(Vec<Self>),
  Any(Vec<Self>),
  Not(Box<Self>),
  Text(Field, Text),
  Ip(IpRanges),
  Score(NumberMatch),
}
#[derive(Debug)]
pub(crate) enum Field {
  Path,
  Method,
  UserAgent,
  Header(HeaderName),
}
#[derive(Debug)]
pub(crate) struct Text {
  source: StringMatch,
  regex: Option<Regex>,
}
#[derive(Default)]
pub(crate) struct Budget {
  nodes: usize,
  bytes: usize,
}
impl Budget {
  fn consume(&mut self, depth: usize) -> Result<()> {
    self.nodes += 1;
    ensure!(depth <= 16, "condition depth must not exceed 16");
    ensure!(
      self.nodes <= 512,
      "policy must not exceed 512 condition nodes"
    );
    Ok(())
  }
}
impl Compiled {
  pub(crate) fn compile(condition: &Condition, depth: usize, budget: &mut Budget) -> Result<Self> {
    budget.consume(depth)?;
    let mut nodes = Vec::new();
    for (values, any) in [(&condition.all, false), (&condition.any, true)] {
      if let Some(values) = values {
        ensure!(
          !values.is_empty(),
          "all/any must contain at least one condition"
        );
        let children = values
          .iter()
          .map(|c| Self::compile(c, depth + 1, budget))
          .collect::<Result<Vec<_>>>()?;
        nodes.push(if any {
          Self::Any(children)
        } else {
          Self::All(children)
        });
      }
    }
    if let Some(value) = &condition.not {
      nodes.push(Self::Not(Box::new(Self::compile(
        value,
        depth + 1,
        budget,
      )?)));
    }
    let mut fields = vec![
      (Field::Path, &condition.path),
      (Field::Method, &condition.method),
      (Field::UserAgent, &condition.user_agent),
    ];
    for (field, source) in fields.drain(..) {
      if let Some(source) = source {
        budget.consume(depth)?;
        let label = match &field {
          Field::Path => "path",
          Field::Method => "method",
          Field::UserAgent => "user_agent",
          Field::Header(_) => unreachable!(),
        };
        let test = Text::compile(source, budget).with_context(|| label.to_string())?;
        nodes.push(Self::Text(field, test));
      }
    }
    for (name, source) in &condition.headers {
      budget.consume(depth)?;
      let name =
        HeaderName::from_bytes(name.as_bytes()).context("headers: expected an HTTP header name")?;
      let test = Text::compile(source, budget).with_context(|| format!("headers.{name}"))?;
      nodes.push(Self::Text(Field::Header(name), test));
    }
    if let Some(value) = &condition.client_ip {
      budget.consume(depth)?;
      let network = value
        .cidr
        .parse()
        .context("client_ip.cidr: expected an IPv4 or IPv6 network")?;
      nodes.push(Self::Ip(IpRanges::from_networks([network])));
    }
    if let Some(value) = &condition.score {
      budget.consume(depth)?;
      ensure!(
        value.eq.is_some()
          || value.gt.is_some()
          || value.gte.is_some()
          || value.lt.is_some()
          || value.lte.is_some(),
        "score: expected a numeric comparison"
      );
      let lower = [
        Some(i32::MIN as i64),
        value.eq.map(i64::from),
        value.gte.map(i64::from),
        value.gt.map(|v| i64::from(v) + 1),
      ]
      .into_iter()
      .flatten()
      .max()
      .unwrap();
      let upper = [
        Some(i32::MAX as i64),
        value.eq.map(i64::from),
        value.lte.map(i64::from),
        value.lt.map(|v| i64::from(v) - 1),
      ]
      .into_iter()
      .flatten()
      .min()
      .unwrap();
      ensure!(lower <= upper, "score: comparisons describe an empty range");
      nodes.push(Self::Score(value.clone()));
    }
    ensure!(
      !nodes.is_empty(),
      "condition must contain at least one matcher"
    );
    Ok(Self::All(nodes))
  }
  pub(crate) fn matches(&self, request: &Request<'_>, score: i32) -> bool {
    match self {
      Self::All(nodes) => nodes.iter().all(|c| c.matches(request, score)),
      Self::Any(nodes) => nodes.iter().any(|c| c.matches(request, score)),
      Self::Not(node) => !node.matches(request, score),
      Self::Text(field, test) => {
        let value = match field {
          Field::Path => Some(request.path),
          Field::Method => Some(request.method.as_str()),
          Field::UserAgent => Some(
            request
              .headers
              .get(http::header::USER_AGENT)
              .and_then(|v| v.to_str().ok())
              .unwrap_or_default(),
          ),
          Field::Header(name) => {
            return request
              .headers
              .get_all(name)
              .iter()
              .any(|v| v.to_str().is_ok_and(|v| test.matches(v)));
          }
        };
        value.is_some_and(|v| test.matches(v))
      }
      Self::Ip(ranges) => request.client_ip.is_some_and(|ip| ranges.contains(ip)),
      Self::Score(test) => {
        test.eq.is_none_or(|v| score == v)
          && test.gt.is_none_or(|v| score > v)
          && test.gte.is_none_or(|v| score >= v)
          && test.lt.is_none_or(|v| score < v)
          && test.lte.is_none_or(|v| score <= v)
      }
    }
  }
}
impl Text {
  fn compile(source: &StringMatch, budget: &mut Budget) -> Result<Self> {
    let count = [
      source.exact.is_some(),
      source.prefix.is_some(),
      source.contains.is_some(),
      source.values.is_some(),
      source.regex.is_some(),
    ]
    .into_iter()
    .filter(|v| *v)
    .count();
    ensure!(
      count == 1,
      "string matcher requires exactly one of exact, prefix, contains, in, regex"
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
  fn matches(&self, value: &str) -> bool {
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
  }
}
