use crate::config::I18nConfig;
use anyhow::{Result, ensure};

/// The locales this deployment can serve, resolved once when the web snapshot is
/// built.
///
/// Every tag here is already normalized, so serving a request never re-parses
/// static configuration. When `web.i18n.supported_locales` is left empty the set
/// comes from the translation files that exist, so adding `i18n/vi.json` is
/// enough to offer Vietnamese. Declaring the list instead restricts it, which is
/// how a deployment keeps a translated locale from being advertised.
#[derive(Debug, Clone)]
pub struct SupportedLocales {
  tags: Vec<String>,
  default_locale: String,
}

impl SupportedLocales {
  /// Resolves the served set from the declared list, or from the translations
  /// available when no list was declared.
  pub fn resolve<'a>(
    config: &I18nConfig,
    available: impl IntoIterator<Item = &'a String>,
  ) -> Result<Self> {
    let default_locale = normalize(&config.default_locale)?;

    let tags = if config.supported_locales.is_empty() {
      let mut tags = available.into_iter().cloned().collect::<Vec<String>>();
      tags.sort();
      tags
    } else {
      config
        .supported_locales
        .iter()
        .map(|tag| normalize(tag))
        .collect::<Result<Vec<String>>>()?
    };

    ensure!(!tags.is_empty(), "No translation files were found");
    ensure!(
      tags.contains(&default_locale),
      "web.i18n.default_locale {} has no translations",
      default_locale
    );
    ensure!(
      tags.windows(2).all(|pair| pair[0] != pair[1]),
      "web.i18n.supported_locales must not repeat a locale"
    );

    Ok(Self {
      tags,
      default_locale,
    })
  }

  pub fn tags(&self) -> &[String] {
    &self.tags
  }

  /// Picks the best locale for one request.
  ///
  /// Precedence is an explicit `lang` parameter, then the language cookie, then
  /// `Accept-Language`, then the default.
  pub fn negotiate(
    &self,
    explicit: Option<&str>,
    cookie: Option<&str>,
    accept: Option<&str>,
  ) -> String {
    let matched = |tag: &str| self.match_parents(tag);

    for candidate in [explicit, cookie].into_iter().flatten() {
      if let Some(locale) = matched(candidate) {
        return locale;
      }
    }

    let mut requested = Vec::new();
    for (index, entry) in accept.unwrap_or("").split(',').take(64).enumerate() {
      let mut parts = entry.trim().split(';');
      let tag = parts.next().unwrap_or("").trim();
      let mut quality = 1.0_f32;
      for parameter in parts {
        let parameter = parameter.trim();
        if let Some(q) = parameter.strip_prefix("q=") {
          quality = q.parse::<f32>().unwrap_or(0.0);
        }
      }
      if quality.is_finite() && quality > 0.0 && quality <= 1.0 {
        requested.push((tag, quality, index));
      }
    }
    requested.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2.cmp(&b.2)));
    for (tag, _, _) in requested {
      if tag == "*" {
        return self.default_locale.clone();
      }
      if let Some(locale) = matched(tag) {
        return locale;
      }
    }

    self.default_locale.clone()
  }

  /// Resolves a requested tag against the served set by walking its parent chain.
  ///
  /// The chain is walked in place rather than collected, so matching a candidate
  /// allocates only the returned tag.
  fn match_parents(&self, tag: &str) -> Option<String> {
    let Ok(canonical) = normalize(tag) else {
      return None;
    };
    let mut current = canonical.as_str();
    loop {
      if let Some(found) = self
        .tags
        .iter()
        .find(|supported| supported.as_str() == current)
      {
        return Some(found.clone());
      }
      let (prefix, _) = current.rsplit_once('-')?;
      current = prefix;
      // A language-range fallback must not end with an extension singleton.
      if current
        .rsplit('-')
        .next()
        .is_some_and(|part| part.len() == 1)
      {
        current = current.rsplit_once('-').map_or(current, |(head, _)| head);
      }
    }
  }
}

pub fn normalize(tag: &str) -> Result<String> {
  ensure!(
    !tag.is_empty() && tag.len() <= 128,
    "Invalid locale tag: {tag}"
  );
  Ok(language_tags::LanguageTag::parse(tag)?.into_string())
}

pub fn parents(tag: &str) -> Vec<String> {
  let mut result = vec![tag.to_string()];
  let mut parent = tag;
  while let Some((prefix, _)) = parent.rsplit_once('-') {
    parent = prefix;
    // A language-range fallback must not end with an extension singleton.
    if parent.rsplit('-').next().is_some_and(|p| p.len() == 1) {
      parent = parent.rsplit_once('-').map_or(parent, |(prefix, _)| prefix);
    }
    result.push(parent.to_string());
  }
  result
}
