use super::{Budget, Compiled, Request, Truth};
use crate::host_load::HostLoad;
use crate::policy::schema::{LoadMatch, LoadWindow, NumberMatch, RateMatch};
use anyhow::{Context, Result, ensure};

/// Inclusive and exclusive bounds that a number must meet.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bounds<T> {
  pub(crate) eq: Option<T>,
  pub(crate) gt: Option<T>,
  pub(crate) gte: Option<T>,
  pub(crate) lt: Option<T>,
  pub(crate) lte: Option<T>,
}
impl<T> Bounds<T> {
  pub(crate) fn is_unbounded(&self) -> bool {
    self.eq.is_none()
      && self.gt.is_none()
      && self.gte.is_none()
      && self.lt.is_none()
      && self.lte.is_none()
  }
}
impl<T: PartialOrd + Copy> Bounds<T> {
  pub(crate) fn contains(&self, value: T) -> bool {
    self.eq.is_none_or(|v| value == v)
      && self.gt.is_none_or(|v| value > v)
      && self.gte.is_none_or(|v| value >= v)
      && self.lt.is_none_or(|v| value < v)
      && self.lte.is_none_or(|v| value <= v)
  }
}
impl From<&NumberMatch> for Bounds<i32> {
  fn from(value: &NumberMatch) -> Self {
    Self {
      eq: value.eq,
      gt: value.gt,
      gte: value.gte,
      lt: value.lt,
      lte: value.lte,
    }
  }
}
impl From<&RateMatch> for Bounds<u32> {
  fn from(value: &RateMatch) -> Self {
    Self {
      eq: value.eq,
      gt: value.gt,
      gte: value.gte,
      lt: value.lt,
      lte: value.lte,
    }
  }
}
impl From<&LoadMatch> for Bounds<f64> {
  fn from(value: &LoadMatch) -> Self {
    Self {
      eq: None,
      gt: value.gt,
      gte: value.gte,
      lt: value.lt,
      lte: value.lte,
    }
  }
}

/// Checks that integer bounds are present and describe a non-empty range within `min..=max`.
fn check_integer_bounds<T: Into<i64> + Copy>(
  bounds: &Bounds<T>,
  min: i64,
  max: i64,
  label: &str,
) -> Result<()> {
  ensure!(
    !bounds.is_unbounded(),
    "{label}: expected a numeric comparison"
  );
  let widen = |value: T| Into::<i64>::into(value);
  let lower = [
    Some(min),
    bounds.eq.map(widen),
    bounds.gte.map(widen),
    bounds.gt.map(|v| widen(v) + 1),
  ]
  .into_iter()
  .flatten()
  .max()
  .unwrap();
  let upper = [
    Some(max),
    bounds.eq.map(widen),
    bounds.lte.map(widen),
    bounds.lt.map(|v| widen(v) - 1),
  ]
  .into_iter()
  .flatten()
  .min()
  .unwrap();
  ensure!(
    lower <= upper,
    "{label}: comparisons describe an empty range"
  );
  Ok(())
}

fn check_load_bounds(bounds: &Bounds<f64>, label: &str) -> Result<()> {
  let limits = [bounds.gt, bounds.gte, bounds.lt, bounds.lte];
  ensure!(
    limits.iter().any(Option::is_some),
    "{label}: expected a numeric comparison"
  );
  ensure!(
    limits.iter().flatten().all(|v| v.is_finite() && *v >= 0.0),
    "{label}: expected a finite number of at least 0"
  );
  // Each bound is a (value, strict) pair; the tightest bound wins and a strict
  // bound is tighter than an inclusive one at the same value.
  let lower = [(bounds.gt, true), (bounds.gte, false)]
    .into_iter()
    .filter_map(|(v, strict)| v.map(|v| (v, strict)))
    .max_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
  let upper = [(bounds.lt, true), (bounds.lte, false)]
    .into_iter()
    .filter_map(|(v, strict)| v.map(|v| (v, strict)))
    .min_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
  if let (Some((low, low_strict)), Some((high, high_strict))) = (lower, upper) {
    ensure!(
      low < high || (low == high && !low_strict && !high_strict),
      "{label}: comparisons describe an empty range"
    );
  }
  Ok(())
}

pub(super) fn compile_score(
  value: &NumberMatch,
  path: &str,
  depth: usize,
  budget: &mut Budget,
) -> Result<Compiled> {
  let label = format!("{path}.score");
  budget.require_rule(&label, "score", false)?;
  budget.consume(depth, path)?;
  let bounds = Bounds::from(value);
  check_integer_bounds(&bounds, i32::MIN.into(), i32::MAX.into(), &label)?;
  Ok(Compiled::Score(bounds))
}

pub(super) fn compile_rate(
  value: &RateMatch,
  path: &str,
  depth: usize,
  budget: &mut Budget,
) -> Result<Compiled> {
  let label = format!("{path}.rate");
  budget.require_rule(&label, "rate", false)?;
  let index = budget
    .counters
    .iter()
    .position(|name| name == &value.counter)
    .with_context(|| format!("{label}.counter: unknown counter {}", value.counter))?;
  budget.consume(depth, path)?;
  let bounds = Bounds::from(value);
  check_integer_bounds(&bounds, 0, u32::MAX.into(), &label)?;
  Ok(Compiled::Rate(index, bounds))
}

pub(super) fn compile_load(
  value: &LoadMatch,
  path: &str,
  depth: usize,
  budget: &mut Budget,
) -> Result<Compiled> {
  budget.consume(depth, path)?;
  let bounds = Bounds::from(value);
  check_load_bounds(&bounds, &format!("{path}.load"))?;
  Ok(Compiled::Load(value.window, bounds))
}

pub(super) fn match_rate(index: usize, bounds: &Bounds<u32>, request: &Request<'_>) -> Truth {
  match request.lookups.rates.get(index).copied().flatten() {
    None => Truth::Unknown,
    Some(count) => Truth::from_bool(bounds.contains(count)),
  }
}

pub(super) fn match_load(window: LoadWindow, bounds: &Bounds<f64>, load: &HostLoad) -> Truth {
  match load.get(window) {
    None => Truth::Unknown,
    Some(value) => Truth::from_bool(bounds.contains(value)),
  }
}
