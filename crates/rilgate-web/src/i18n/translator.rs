use super::store::TranslationStore;
use minijinja::{Environment, Error, State, value::Kwargs};
use std::{collections::BTreeMap, sync::Arc};

pub fn register(env: &mut Environment<'static>, store: Arc<TranslationStore>) {
  let lookup_store = store.clone();
  env.add_function("has_t", move |state: &State<'_, '_>, key: String| {
    let locale = state
      .lookup("locale")
      .and_then(|value| value.as_str().map(str::to_string))
      .unwrap_or_else(|| lookup_store.default_locale.clone());
    lookup_store.has(&locale, &key)
  });
  env.add_function(
    "t",
    move |state: &State<'_, '_>, key: String, kwargs: Kwargs| -> Result<String, Error> {
      let locale = state
        .lookup("locale")
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| store.default_locale.clone());
      let mut args = BTreeMap::new();
      for name in kwargs.args() {
        args.insert(
          name.to_string(),
          kwargs.get::<minijinja::Value>(name)?.to_string(),
        );
      }
      kwargs.assert_all_used()?;
      store.translate(&locale, &key, &args)
    },
  );
}
